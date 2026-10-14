//! libmpv 运行时绑定与纯音频播放引擎。
//!
//! ## 设计
//!
//! - **运行时动态加载**（`libloading`）：libmpv 缺失时返回错误，由上层降级为「不出声」，
//!   不影响应用其余功能。因此本模块**不链接** libmpv，也不需要导入库。
//! - **无平台判断**：库名与搜索路径由调用方以候选列表传入
//!   （见 `siren-platform` 的 `NativeLibraryLocator`），本模块只负责 `extern "C"` 调用。
//!
//! 仅映射 `mpv/client.h` 的最小 API 集合（对照 Melodia 的 `LibMpv.kt`）。

use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};

// mpv_format
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_DOUBLE: c_int = 5;

type MpvCreate = unsafe extern "C" fn() -> *mut c_void;
type MpvInitialize = unsafe extern "C" fn(*mut c_void) -> c_int;
type MpvSetOptionString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type MpvSetPropertyString = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type MpvGetProperty = unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut c_void) -> c_int;
type MpvGetPropertyString = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type MpvCommand = unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int;
type MpvFree = unsafe extern "C" fn(*mut c_void);
type MpvTerminateDestroy = unsafe extern "C" fn(*mut c_void);
type MpvErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

/// 音频输出设备。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    /// mpv 内部设备名（如 `wasapi/{...}` / `pipewire/...`）。
    pub name: String,
    /// 面向用户的描述。
    pub description: String,
}

/// 已加载的 libmpv 函数表，持有 `Library` 以延长其生命周期。
struct MpvLib {
    _lib: Library,
    create: MpvCreate,
    initialize: MpvInitialize,
    set_option_string: MpvSetOptionString,
    set_property_string: MpvSetPropertyString,
    get_property: MpvGetProperty,
    get_property_string: MpvGetPropertyString,
    command: MpvCommand,
    free: MpvFree,
    terminate_destroy: MpvTerminateDestroy,
    error_string: MpvErrorString,
}

impl MpvLib {
    /// 依次尝试候选库名/路径，返回首个加载成功者。
    fn load(candidates: &[String]) -> Result<Self, String> {
        let mut last_error = String::from("未提供任何 libmpv 候选路径");
        for candidate in candidates {
            match unsafe { Library::new(candidate) } {
                Ok(lib) => return unsafe { Self::from_library(lib) },
                Err(e) => last_error = format!("{candidate}: {e}"),
            }
        }
        Err(last_error)
    }

    unsafe fn from_library(lib: Library) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                let symbol: Symbol<$ty> = lib.get($name).map_err(|e| e.to_string())?;
                *symbol
            }};
        }
        Ok(MpvLib {
            create: sym!(b"mpv_create", MpvCreate),
            initialize: sym!(b"mpv_initialize", MpvInitialize),
            set_option_string: sym!(b"mpv_set_option_string", MpvSetOptionString),
            set_property_string: sym!(b"mpv_set_property_string", MpvSetPropertyString),
            get_property: sym!(b"mpv_get_property", MpvGetProperty),
            get_property_string: sym!(b"mpv_get_property_string", MpvGetPropertyString),
            command: sym!(b"mpv_command", MpvCommand),
            free: sym!(b"mpv_free", MpvFree),
            terminate_destroy: sym!(b"mpv_terminate_destroy", MpvTerminateDestroy),
            error_string: sym!(b"mpv_error_string", MpvErrorString),
            _lib: lib,
        })
    }
}

/// 纯音频播放引擎。
///
/// 通过 [`Mpv::load`] 传入候选库列表创建；`Drop` 时释放 mpv 句柄。
pub struct Mpv {
    lib: MpvLib,
    handle: *mut c_void,
    alive: AtomicBool,
}

// mpv 的 client API 线程安全；句柄本身不含 `!Send/!Sync` 数据。
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

impl Mpv {
    /// 从候选库名/路径加载并初始化引擎。
    pub fn load(candidates: &[String]) -> Result<Self, String> {
        let lib = MpvLib::load(candidates)?;
        unsafe {
            let handle = (lib.create)();
            if handle.is_null() {
                return Err("mpv_create 返回空句柄（常见原因：进程 locale 非 C）".to_string());
            }
            let mpv = Mpv {
                lib,
                handle,
                alive: AtomicBool::new(true),
            };
            // 纯音频：关闭视频与终端输出
            mpv.set_option("vid", "no");
            mpv.set_option("video", "no");
            mpv.set_option("audio-display", "no");
            mpv.set_option("terminal", "no");
            mpv.set_option("idle", "yes");
            mpv.set_option("keep-open", "yes");

            let rc = (mpv.lib.initialize)(handle);
            if rc < 0 {
                return Err(format!("mpv_initialize 失败: {}", mpv.error(rc)));
            }
            Ok(mpv)
        }
    }

    /// 加载并播放（或预载）地址。
    pub fn load_file(&self, url: &str) {
        self.command(&["loadfile", url, "replace"]);
    }

    /// 加载地址并从 `start_ms` 开始（借助 `loadfile` 的 `start=` 选项，加载完成后自动跳转）。
    pub fn load_file_from(&self, url: &str, start_ms: Option<i64>) {
        match start_ms {
            Some(ms) if ms > 0 => {
                let opt = format!("start={:.3}", ms as f64 / 1000.0);
                self.command(&["loadfile", url, "replace", "-1", &opt]);
            }
            _ => self.load_file(url),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.set_property("pause", if paused { "yes" } else { "no" });
    }

    pub fn seek_to_ms(&self, position_ms: i64) {
        let seconds = (position_ms.max(0) as f64) / 1000.0;
        self.command(&["seek", &seconds.to_string(), "absolute"]);
    }

    /// 音量百分比（0.0..=100.0）。
    pub fn set_volume(&self, percent: f64) {
        let clamped = percent.clamp(0.0, 100.0);
        self.set_property("volume", &format!("{clamped:.3}"));
    }

    pub fn stop(&self) {
        self.command(&["stop"]);
    }

    /// 当前位置（秒）；未加载时为 `None`。
    pub fn position(&self) -> Option<f64> {
        self.get_property_double("time-pos")
    }

    /// 总时长（秒）；未加载时为 `None`。
    pub fn duration(&self) -> Option<f64> {
        self.get_property_double("duration")
    }

    /// 是否暂停（`pause` 属性）。
    pub fn is_paused(&self) -> Option<bool> {
        self.get_property_flag("pause")
    }

    /// 是否已到文件结尾（`eof-reached` 属性），用于自然播完后自动切歌。
    pub fn eof_reached(&self) -> Option<bool> {
        self.get_property_flag("eof-reached")
    }

    /// 音频输出设备列表（解析 `audio-device-list`）。
    pub fn audio_devices(&self) -> Vec<AudioDevice> {
        self.get_property_string("audio-device-list")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 切换音频输出设备（失败时原设备不变）。
    pub fn set_audio_device(&self, name: &str) {
        self.set_property("audio-device", name);
    }

    pub fn set_option(&self, name: &str, value: &str) {
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        unsafe {
            (self.lib.set_option_string)(self.handle, name.as_ptr(), value.as_ptr());
        }
    }

    pub fn set_property(&self, name: &str, value: &str) {
        if !self.alive.load(Ordering::Relaxed) {
            return;
        }
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        unsafe {
            (self.lib.set_property_string)(self.handle, name.as_ptr(), value.as_ptr());
        }
    }

    fn get_property_double(&self, name: &str) -> Option<f64> {
        if !self.alive.load(Ordering::Relaxed) {
            return None;
        }
        let name = CString::new(name).ok()?;
        let mut value: c_double = 0.0;
        let rc = unsafe {
            (self.lib.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                &mut value as *mut c_double as *mut c_void,
            )
        };
        (rc >= 0).then_some(value)
    }

    fn get_property_flag(&self, name: &str) -> Option<bool> {
        if !self.alive.load(Ordering::Relaxed) {
            return None;
        }
        let name = CString::new(name).ok()?;
        let mut value: c_int = 0;
        let rc = unsafe {
            (self.lib.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                &mut value as *mut c_int as *mut c_void,
            )
        };
        (rc >= 0).then_some(value != 0)
    }

    fn get_property_string(&self, name: &str) -> Option<String> {
        if !self.alive.load(Ordering::Relaxed) {
            return None;
        }
        let name = CString::new(name).ok()?;
        let ptr = unsafe { (self.lib.get_property_string)(self.handle, name.as_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        unsafe { (self.lib.free)(ptr as *mut c_void) };
        Some(text)
    }

    fn command(&self, args: &[&str]) {
        if !self.alive.load(Ordering::Relaxed) {
            return;
        }
        let cstrings: Vec<CString> = args.iter().filter_map(|a| CString::new(*a).ok()).collect();
        if cstrings.len() != args.len() {
            return;
        }
        let mut ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        unsafe {
            (self.lib.command)(self.handle, ptrs.as_ptr());
        }
    }

    fn error(&self, code: c_int) -> String {
        unsafe {
            let ptr = (self.lib.error_string)(code);
            if ptr.is_null() {
                return format!("error {code}");
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if self.alive.swap(false, Ordering::SeqCst) {
            unsafe {
                (self.lib.terminate_destroy)(self.handle);
            }
        }
    }
}

impl crate::player::PlaybackController for Mpv {
    fn load(&self, url: &str) {
        self.load_file(url);
    }

    fn load_from(&self, url: &str, start_ms: Option<i64>) {
        self.load_file_from(url, start_ms);
    }

    fn set_paused(&self, paused: bool) {
        Mpv::set_paused(self, paused);
    }

    fn stop(&self) {
        Mpv::stop(self);
    }

    fn seek_to_ms(&self, position_ms: i64) {
        Mpv::seek_to_ms(self, position_ms);
    }

    fn set_volume(&self, percent: f64) {
        Mpv::set_volume(self, percent);
    }

    fn position_ms(&self) -> Option<i64> {
        self.position().map(|s| (s * 1000.0) as i64)
    }

    fn duration_ms(&self) -> Option<i64> {
        self.duration().map(|s| (s * 1000.0) as i64)
    }

    fn is_paused(&self) -> Option<bool> {
        Mpv::is_paused(self)
    }

    fn eof_reached(&self) -> Option<bool> {
        Mpv::eof_reached(self)
    }
}
