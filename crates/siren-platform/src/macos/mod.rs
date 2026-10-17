//! macOS 平台实现。
//!
//! | 能力 | 方案 |
//! |---|---|
//! | 目录 | `directories` crate |
//! | 媒体控制 | `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter`（经 playwire，跨平台代码在根级） |
//! | 托盘 | `tray-icon` crate（`NSStatusItem`，代码在根级） |
//! | 窗口系统 | Cocoa / AppKit（当前降级；待补） |
//! | 单实例 | 当前占位放行（待补 `NSRunningApplication` + 激活） |
//! | 快捷键 / 自启 | 计划 `global-hotkey` / `auto-launch`（未接入） |
//! | 窗口装饰 / 强调色 / 歌词穿透 | 待补（`NSWindow` / `NSColor`） |
//!
//! ## 说明
//!
//! macOS 支持**可能排在首期之后**（见 `docs/06-路线图.md` 阶段 4）。
//! 但保留此目录，以确保平台抽象从一开始就是三平台的，
//! 而非事后补丁式地加入第三个平台。
//!
//! 本模块不对外可见：上层仅经 `lib.rs`「四、统一导出」调用；
//! 根级接口/跨平台文件经 `crate::imp::`（见 `lib.rs`「三」）取用下列入口。
//!
//! ## TODO
//!
//! pub mod accent;     // NSColor
//! pub mod lyric;      // NSWindow

mod audio;
mod native;
mod shell;
mod single_instance;
mod window_system;

use crate::native_library::NativeLibraryLocator;
use crate::window_system::WindowSystem;

/// 当前平台的窗口系统实现（当前降级）。
pub(crate) fn window_system() -> &'static dyn WindowSystem {
    &self::window_system::MacosWindowSystem
}

/// 当前平台的原生库定位实现。
pub(crate) fn native_library() -> &'static dyn NativeLibraryLocator {
    &native::MacosNativeLibraryLocator
}

pub(crate) use audio::device_prefixes;

/// macOS：暂未接入 `task_info`，返回 `None`（设置页显示「—」）。
pub(crate) fn rss_bytes() -> Option<u64> {
    None
}
pub(crate) use shell::open_url;
pub(crate) use single_instance::{ensure_single_instance, poll_activation_events};

/// macOS 使用系统原生装饰（CSD 待补）。
pub(crate) fn prefers_client_decorations() -> bool {
    false
}
