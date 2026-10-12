//! # siren-platform
//!
//! 平台能力层：**唯一允许出现平台差异的 crate**。
//!
//! ## 组织方式
//!
//! - `src/*.rs`（根级）：平台能力 **trait 定义**，与平台无关
//! - `src/windows/`、`src/linux/`、`src/macos/`：各平台 **实现**
//!
//! ## 编译期保证
//!
//! 通过 `#[cfg(target_os = "...")]` 在编译期选择实现。
//! 若新增目标平台而未提供实现，会因符号未定义而**编译失败**——
//! 这是 Melodia 需要自建 KSP 处理器才能达到的效果，在 Rust 中是语言特性。
//!
//! ## 能力清单
//!
//! 详见 `docs/02-平台能力规格.md`。

// ── 平台能力 trait（与平台无关，始终编译）────────────────────────
pub mod accent;
pub mod app_paths;
pub mod audio;
pub mod lyric_window;
pub mod media_session;
pub mod native_library;
pub mod ui_scale;
pub mod window;
// pub mod tray;          // TODO: 若 tray-icon 不够用再自建
// pub mod hotkey;        // TODO: 优先用 global-hotkey crate
// pub mod autostart;     // TODO: 优先用 auto-launch crate

// ── 平台实现（由 cfg 选择）───────────────────────────────────────
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

// ── 平台类型统一导出 ─────────────────────────────────────────────
//
// 上层通过这里的别名取用当前平台的实现，无需关心具体平台。

#[cfg(target_os = "linux")]
pub use linux::native::LinuxNativeLibraryLocator as PlatformNativeLibrary;
#[cfg(target_os = "macos")]
pub use macos::native::MacosNativeLibraryLocator as PlatformNativeLibrary;
/// 当前平台的原生库定位实现。
#[cfg(target_os = "windows")]
pub use windows::native::WindowsNativeLibraryLocator as PlatformNativeLibrary;

/// 当前平台的 libmpv 库名/路径候选（便捷入口）。
pub fn mpv_library_candidates() -> Vec<String> {
    use native_library::NativeLibraryLocator as _;
    PlatformNativeLibrary.mpv_candidates()
}

/// 加载 libmpv 前的一次性准备（便捷入口，如纠正数值区域）。
pub fn prepare_native_environment() {
    use native_library::NativeLibraryLocator as _;
    PlatformNativeLibrary.prepare();
}

/// 当前平台的系统媒体控制实现（playwire 后端，跨平台）。
pub use media_session::PlaywireMediaSession as PlatformMediaSession;
pub use media_session::{MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlaybackStatus};

#[cfg(target_os = "linux")]
pub use linux::media::hwnd_from as platform_hwnd_from;
#[cfg(target_os = "macos")]
pub use macos::media::hwnd_from as platform_hwnd_from;
/// 当前平台的窗口句柄解析（Windows SMTC 需要）。
#[cfg(target_os = "windows")]
pub use windows::media::hwnd_from as platform_hwnd_from;

/// 从任意实现了 `raw-window-handle` 的窗口解析原生句柄。
///
/// 上层（如 GPUI 的 `Window`）无需依赖 `raw-window-handle` 类型即可调用。
pub fn window_hwnd<W: raw_window_handle::HasWindowHandle + ?Sized>(window: &W) -> Option<u64> {
    let handle = window.window_handle().ok().map(|handle| handle.as_raw());
    platform_hwnd_from(handle)
}

/// 注册进程级 AppUserModelID（Windows 用于媒体浮层显示应用名；其他平台无操作）。
#[cfg(target_os = "windows")]
pub use windows::appmodel::set_app_user_model_id;
#[cfg(not(target_os = "windows"))]
pub fn set_app_user_model_id(_id: &str) {}

/// 主显示器工作区（逻辑像素 `x,y,w,h`；非 Windows 返回 None）。
#[cfg(target_os = "windows")]
pub use windows::display::primary_work_area;
#[cfg(not(target_os = "windows"))]
pub fn primary_work_area() -> Option<(f32, f32, f32, f32)> {
    None
}

/// 主显示器工作区（物理像素；非 Windows 返回 None）。
#[cfg(target_os = "windows")]
pub use windows::display::primary_work_area_physical;
#[cfg(not(target_os = "windows"))]
pub fn primary_work_area_physical() -> Option<(i32, i32, i32, i32)> {
    None
}

/// 将窗口移动到物理坐标（非 Windows 无操作）。
#[cfg(target_os = "windows")]
pub use windows::display::move_window;
#[cfg(not(target_os = "windows"))]
pub fn move_window(_hwnd: u64, _x: i32, _y: i32, _w: i32, _h: i32) {}

/// 窗口的物理矩形 `(x,y,w,h)`；非 Windows 返回 None。
#[cfg(target_os = "windows")]
pub use windows::display::window_physical_rect;
#[cfg(not(target_os = "windows"))]
pub fn window_physical_rect(_hwnd: u64) -> Option<(i32, i32, i32, i32)> {
    None
}

/// 用系统默认程序打开 URL（如网页登录）。
#[cfg(target_os = "windows")]
pub fn open_url(url: &str) {
    // `start` 的首个带引号参数会被当作窗口标题，故传入空标题占位。
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
}
#[cfg(target_os = "linux")]
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
#[cfg(target_os = "macos")]
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).spawn();
}
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
pub fn open_url(_url: &str) {}

/// 编译期平台断言。
///
/// 若目标平台不在支持列表内，此模块会触发编译错误，
/// 避免出现"能编译但运行期无实现"的情况。
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
compile_error!(
    "Siren 目前仅支持 Windows / Linux / macOS。\
     若需支持新平台，请在 crates/siren-platform/src/ 下新增对应模块，\
     并在 lib.rs 中补充 cfg 分支与统一导出。"
);
