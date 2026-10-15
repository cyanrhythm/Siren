//! Windows 平台实现。
//!
//! | 能力 | 方案 |
//! |---|---|
//! | 目录 | `directories` crate（跨平台，无需平台特定代码） |
//! | 媒体控制 | **SMTC**（经 playwire，跨平台代码在根级 `media_session.rs`） |
//! | 托盘 | `tray-icon` crate（跨平台，代码在根级 `tray.rs`） |
//! | 窗口系统 | Win32（`display` / `icon` / `appmodel`） |
//! | 单实例 | 命名互斥体 + `FindWindowW` 唤起 |
//! | 快捷键 / 自启 | 计划 `global-hotkey` / `auto-launch`（未接入） |
//!
//! 本模块不对外可见：上层仅经 `lib.rs`「四、统一导出」调用；
//! 根级接口/跨平台文件经 `crate::imp::`（见 `lib.rs`「三」）取用下列入口。

mod appmodel;
mod audio;
mod display;
mod icon;
mod native;
mod shell;
mod single_instance;
mod window_system;

use crate::native_library::NativeLibraryLocator;
use crate::window_system::WindowSystem;

/// 当前平台的窗口系统实现。
pub(crate) fn window_system() -> &'static dyn WindowSystem {
    &self::window_system::WindowsWindowSystem
}

/// 当前平台的原生库定位实现。
pub(crate) fn native_library() -> &'static dyn NativeLibraryLocator {
    &native::WindowsNativeLibraryLocator
}

pub(crate) use audio::device_prefixes;
pub(crate) use shell::open_url;
pub(crate) use single_instance::{ensure_single_instance, poll_activation_events};

/// Windows 隐藏系统标题栏（`appears_transparent`）+ NC 命中测试，不请求 CSD。
pub(crate) fn prefers_client_decorations() -> bool {
    false
}
