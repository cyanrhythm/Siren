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
    /// 串行化全部 FFI 调用与 [`Drop`]，消除「检查 alive 之后、调用之前被销毁」的 TOCTOU。
    lock: std::sync::Mutex<()>,
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
                lock: std::sync::Mutex::new(()),
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

    /// 加载并播放（或预载）地址；返回 libmpv 是否接受该命令。
    pub fn load_file(&self, url: &str) -> Result<(), String> {
        self.command(&["loadfile", url, "replace"])
    }

    /// 加载地址并从 `start_ms` 开始（借助 `loadfile` 的 `start=` 选项，加载完成后自动跳转）。
    ///
    /// **需要 mpv ≥ 0.38**：`loadfile` 的 `<index>=-1` 参数在旧版不被识别，会返回错误码。
    /// 返回 `Err` 时上层不应把状态置为「播放中」（否则成为无声的假播放）。
    pub fn load_file_from(&self, url: &str, start_ms: Option<i64>) -> Result<(), String> {
        match start_option(start_ms) {
            Some(opt) => self.command(&["loadfile", url, "replace", "-1", &opt]),
            None => self.load_file(url),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.set_property("pause", if paused { "yes" } else { "no" });
    }

    pub fn seek_to_ms(&self, position_ms: i64) {
        let seconds = (position_ms.max(0) as f64) / 1000.0;
        let _ = self.command(&["seek", &seconds.to_string(), "absolute"]);
    }

    /// 音量百分比（0.0..=100.0）。
    pub fn set_volume(&self, percent: f64) {
        self.set_property("volume", &volume_property(percent));
    }

    pub fn stop(&self) {
        let _ = self.command(&["stop"]);
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

    /// 持锁并确认句柄存活后，用原始句柄执行 `f`；句柄已销毁时返回 `None`。
    ///
    /// 所有触碰 `handle` 的 FFI 都经此进入，与 [`Drop`] 互斥，杜绝 UAF / TOCTOU。
    fn with_handle<R>(&self, f: impl FnOnce(*mut c_void) -> R) -> Option<R> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if !self.alive.load(Ordering::SeqCst) {
            return None;
        }
        Some(f(self.handle))
    }

    pub fn set_option(&self, name: &str, value: &str) {
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        let _ = self.with_handle(|h| unsafe {
            (self.lib.set_option_string)(h, name.as_ptr(), value.as_ptr());
        });
    }

    pub fn set_property(&self, name: &str, value: &str) {
        let (Ok(name), Ok(value)) = (CString::new(name), CString::new(value)) else {
            return;
        };
        let _ = self.with_handle(|h| unsafe {
            (self.lib.set_property_string)(h, name.as_ptr(), value.as_ptr());
        });
    }

    fn get_property_double(&self, name: &str) -> Option<f64> {
        let name = CString::new(name).ok()?;
        self.with_handle(|h| {
            let mut value: c_double = 0.0;
            let rc = unsafe {
                (self.lib.get_property)(
                    h,
                    name.as_ptr(),
                    MPV_FORMAT_DOUBLE,
                    &mut value as *mut c_double as *mut c_void,
                )
            };
            (rc >= 0).then_some(value)
        })
        .flatten()
    }

    fn get_property_flag(&self, name: &str) -> Option<bool> {
        let name = CString::new(name).ok()?;
        self.with_handle(|h| {
            let mut value: c_int = 0;
            let rc = unsafe {
                (self.lib.get_property)(
                    h,
                    name.as_ptr(),
                    MPV_FORMAT_FLAG,
                    &mut value as *mut c_int as *mut c_void,
                )
            };
            (rc >= 0).then_some(value != 0)
        })
        .flatten()
    }

    fn get_property_string(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        self.with_handle(|h| {
            let ptr = unsafe { (self.lib.get_property_string)(h, name.as_ptr()) };
            if ptr.is_null() {
                return None;
            }
            let text = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
            unsafe { (self.lib.free)(ptr as *mut c_void) };
            Some(text)
        })
        .flatten()
    }

    /// 执行 libmpv 命令；返回返回码是否成功（失败带上 `mpv_error_string`）。
    fn command(&self, args: &[&str]) -> Result<(), String> {
        let cstrings: Vec<CString> = args.iter().filter_map(|a| CString::new(*a).ok()).collect();
        if cstrings.len() != args.len() {
            return Err("命令参数含 NUL".to_string());
        }
        let mut ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(ptr::null());
        match self.with_handle(|h| unsafe { (self.lib.command)(h, ptrs.as_ptr()) }) {
            None => Err("引擎已销毁".to_string()),
            Some(rc) if rc < 0 => Err(self.error(rc)),
            Some(_) => Ok(()),
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

// ── 参数序列化（纯函数：不触碰 FFI，故可单测）────────────────────────
//
// libmpv 的命令/属性参数都是字符串，格式错会静默变成「加载后不跳转」「音量被夹」
// 这类难查的行为差异，因此把「毫秒 → 选项串」的换算单独提出来固定住。

/// `loadfile` 的 `start=` 选项：`None` / 非正值表示从头开始（不追加选项）。
///
/// 保留三位小数（毫秒精度）；`start=0.000` 无意义，故与 `None` 同样处理。
fn start_option(start_ms: Option<i64>) -> Option<String> {
    match start_ms {
        Some(ms) if ms > 0 => Some(format!("start={:.3}", ms as f64 / 1000.0)),
        _ => None,
    }
}

/// 音量属性串：夹到 0..=100 并统一三位小数。
///
/// 非有限值里的 NaN 按 0 处理：`f64::clamp` 对 NaN 会原样返回，
/// 直接格式化就成了 libmpv 不认识的 `"NaN"`（±∞ 由 clamp 正常收敛到边界）。
fn volume_property(percent: f64) -> String {
    let percent = if percent.is_nan() { 0.0 } else { percent };
    format!("{:.3}", percent.clamp(0.0, 100.0))
}

/// libmpv 的秒 → 毫秒（截断，与 `(s * 1000.0) as i64` 语义一致）。
fn seconds_to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0) as i64
}

impl Drop for Mpv {
    fn drop(&mut self) {
        // 持锁销毁：与 `with_handle` 互斥，确保不会在 FFI 调用中途释放句柄。
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.alive.swap(false, Ordering::SeqCst) {
            unsafe {
                (self.lib.terminate_destroy)(self.handle);
            }
        }
    }
}

impl crate::player::PlaybackController for Mpv {
    fn load(&self, url: &str) -> bool {
        Mpv::load_file(self, url).is_ok()
    }

    fn load_from(&self, url: &str, start_ms: Option<i64>) -> bool {
        Mpv::load_file_from(self, url, start_ms).is_ok()
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
        self.position().map(seconds_to_ms)
    }

    fn duration_ms(&self) -> Option<i64> {
        self.duration().map(seconds_to_ms)
    }

    fn is_paused(&self) -> Option<bool> {
        Mpv::is_paused(self)
    }

    fn eof_reached(&self) -> Option<bool> {
        Mpv::eof_reached(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 参数序列化 ────────────────────────────────────────────────
    // 这几条不需要 libmpv：一旦格式漂移，上层会表现成「恢复进度不生效」而非报错。

    #[test]
    fn start_option_keeps_millisecond_precision() {
        assert_eq!(start_option(Some(1500)).as_deref(), Some("start=1.500"));
        assert_eq!(start_option(Some(250)).as_deref(), Some("start=0.250"));
        assert_eq!(start_option(Some(3_600_001)).as_deref(), Some("start=3600.001"));
    }

    #[test]
    fn start_option_omits_non_positive_positions() {
        // `start=0.000` 与「从头开始」等价，没必要追加参数（旧版 mpv 还可能拒绝）。
        assert_eq!(start_option(None), None);
        assert_eq!(start_option(Some(0)), None);
        assert_eq!(start_option(Some(-42)), None);
    }

    #[test]
    fn volume_is_clamped_and_formatted() {
        assert_eq!(volume_property(50.0), "50.000");
        assert_eq!(volume_property(0.0), "0.000");
        assert_eq!(volume_property(100.0), "100.000");
        // 越界值不得原样透传给 libmpv（否则可能被拒绝或产生 >100 的增益）。
        assert_eq!(volume_property(150.0), "100.000");
        assert_eq!(volume_property(-10.0), "0.000");
        // 非有限值不得变成 libmpv 不认识的 "NaN"，按静音处理。
        assert_eq!(volume_property(f64::NAN), "0.000");
        assert_eq!(volume_property(f64::INFINITY), "100.000");
    }

    #[test]
    fn seconds_convert_to_ms_by_truncation() {
        assert_eq!(seconds_to_ms(0.0), 0);
        assert_eq!(seconds_to_ms(1.5), 1500);
        assert_eq!(seconds_to_ms(2.9999), 2999);
        assert_eq!(seconds_to_ms(-1.0), -1000);
    }

    // ── 加载失败降级 ──────────────────────────────────────────────
    // 没有 libmpv 的环境（CI）也能跑：验证「找不到库」是 Err 而不是 panic。

    #[test]
    fn empty_candidate_list_reports_reason() {
        let err = Mpv::load(&[]).err().expect("空候选列表必须失败");
        assert_eq!(err, "未提供任何 libmpv 候选路径");
    }

    #[test]
    fn missing_library_reports_last_candidate() {
        let candidates = vec!["/nonexistent/siren-no-such-libmpv.so.2".to_string()];
        let err = Mpv::load(&candidates).err().expect("不存在的库必须失败");
        // 错误信息要带上是哪个候选，否则用户无法判断是路径还是符号缺失。
        assert!(err.contains("/nonexistent/siren-no-such-libmpv.so.2"), "实际: {err}");
        assert!(!err.is_empty());
    }

    #[test]
    fn load_failure_does_not_panic_and_reports_last_of_many() {
        let candidates = vec!["/nonexistent/a.so".to_string(), "/nonexistent/b.so".to_string()];
        let err = Mpv::load(&candidates).err().expect("全部候选都不存在时必须失败");
        assert!(err.contains("/nonexistent/b.so"), "应报告最后一个候选，实际: {err}");
    }
}
