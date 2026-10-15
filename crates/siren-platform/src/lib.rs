//! # siren-platform
//!
//! 平台能力层：**唯一允许出现平台差异的 crate**。
//!
//! ## 组织方式
//!
//! - `src/*.rs`（根级）：**接口文件或跨平台实现文件**，与平台无关：
//!   - 接口形态：多操作能力用 trait（[`window_system::WindowSystem`]、
//!     [`native_library::NativeLibraryLocator`]、[`media_session::MediaSession`]）；
//!     单点能力用统一函数契约（[`shell::open_url`]、[`single_instance::ensure_single_instance`]、
//!     [`audio::device_prefixes`]）。
//!   - 跨平台实现：[`tray`]（`tray-icon` crate）、[`media_session`]（playwire）。
//!   - 规格占位（尚未定义接口）：`accent` / `app_paths` / `window` / `lyric_window` / `ui_scale`。
//! - `src/windows/`、`src/linux/`、`src/macos/`：**平台实现**，一一对应根级接口。
//!   模块不对外可见；本文件「三」是**全 crate 唯一的平台选择点**，
//!   「四」的导出不出现任何 `#[cfg(target_os)]`。
//!
//! ## 编译期保证
//!
//! 平台实现缺任何接口方法/函数，都会在对应平台上**编译失败**；
//! 新增目标平台而未提供实现同样如此（见文件末尾断言）。
//!
//! ## 平台实现现状（2026-10）
//!
//! | 能力 | Windows | Linux | macOS |
//! |---|---|---|---|
//! | 原生库定位（libmpv） | ✅ | ✅ | ✅ |
//! | 系统媒体控制（playwire） | ✅ 已验证 | 🟡 KDE/GNOME 待实机验证 | 🟡 待验证 |
//! | 系统托盘（tray-icon） | ✅ | 🟡 ksni 待实机验证 | ⬜ 待验证 |
//! | 窗口系统（显隐/几何/图标/句柄） | ✅ | ⬜ 待补（当前降级） | ⬜ 待补（当前降级） |
//! | 单实例 | ✅ | ⬜ 占位放行 | ⬜ 占位放行 |
//!
//! **降级约定**：上表 ⬜ 能力在对应平台的实现一律 no-op / `None` / `false`，
//! 绝不 panic；上层据返回值决定降级行为（如托盘不可用时关闭即退出）。
//! 全局快捷键 / 开机自启尚未建模（计划接入 `global-hotkey` / `auto-launch`）。
//!
//! ## 能力清单
//!
//! 详见 `docs/02-平台能力规格.md`。

// ── 一、平台能力（根级：接口 / 跨平台实现；始终编译）────────────────
pub mod accent;
pub mod app_paths;
pub mod audio;
pub mod lyric_window;
pub mod media_session;
pub mod native_library;
pub mod shell;
pub mod single_instance;
pub mod tray;
pub mod ui_scale;
pub mod window;
pub mod window_system;

// ── 二、平台实现（由 cfg 选择；不对外，经「三」「四」暴露）──────────
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

// ── 三、当前平台选择（全 crate 唯一的平台分支）──────────────────────
//
// 根级接口/跨平台文件经 `crate::imp::` 调用当前平台实现；
// 各平台模块按约定提供：`window_system()` / `native_library()` /
// `ensure_single_instance` / `open_url` / `device_prefixes`。
#[cfg(target_os = "windows")]
pub(crate) use windows as imp;

#[cfg(target_os = "linux")]
pub(crate) use linux as imp;

#[cfg(target_os = "macos")]
pub(crate) use macos as imp;

// ── 四、统一导出（上层唯一入口；无平台分支）────────────────────────

pub use media_session::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlaybackStatus,
    PlaywireMediaSession as PlatformMediaSession,
};
pub use native_library::{mpv_library_candidates, prepare_native_environment};
pub use shell::open_url;
pub use single_instance::ensure_single_instance;
pub use tray::{create_tray, poll_tray_events, Tray as PlatformTray, TrayEvent};
pub use window_system::{
    hide_window, is_window_minimized, is_window_visible, move_window, primary_work_area, primary_work_area_physical,
    restore_window, set_app_user_model_id, set_window_icon, show_window, toggle_window, window_hwnd,
    window_physical_rect,
};

// ── 五、编译期平台断言 ──────────────────────────────────────────────

/// 若目标平台不在支持列表内，触发编译错误，避免"能编译但运行期无实现"。
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
compile_error!(
    "Siren 目前仅支持 Windows / Linux / macOS。\
     若需支持新平台，请在 crates/siren-platform/src/ 下新增对应平台目录\
     （按同一接口实现 window_system / single_instance / shell / audio 等），\
     并在 lib.rs「三、当前平台选择」登记。"
);
