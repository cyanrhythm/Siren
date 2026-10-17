//! Linux 平台实现。
//!
//! ## 平台优先级
//!
//! **Linux 是主要开发目标，且以 Wayland 会话为优先。**
//!
//! 新功能先在 Wayland 下验证，再补 X11；受 Wayland 协议限制的能力明确降级。
//!
//! ## 会话类型分支
//!
//! X11 与 Wayland 的差异**不是** GUI 工具包能抹平的——
//! Wayland 协议本身不提供全局热键、窗口点击穿透、获取他人窗口句柄等能力。
//!
//! | 能力 | Wayland | X11 |
//! |---|---|---|
//! | 媒体控制（MPRIS） | ✅ D-Bus，与显示协议无关 | ✅ |
//! | 托盘（SNI） | ✅ D-Bus，与显示协议无关 | ✅ |
//! | 单实例 | ✅ flock + D-Bus 激活（`single_instance.rs`） | ✅ 同 |
//! | 窗口显隐 / 几何 | ❌ 无协议，降级（GPUI 最小化/激活兜底） | ⬜ 待补（x11rb） |
//! | 全局快捷键 | ⚠️ 仅 XDG Portal，非所有 DE 支持 | ✅ `XGrabKey` |
//! | 歌词点击穿透 | ❌ 无协议，降级 | ✅ `XShape` |
//! | 窗口装饰 | 交给合成器 | 同 |
//!
//! ## 目录组织
//!
//! - `window_system.rs`：窗口系统实现（Wayland 路线：协议不可及项降级，
//!   上层用 GPUI 窗口 API 兜底；X11 待补）
//! - `single_instance.rs`：单实例（flock + D-Bus `Activate` 唤起）
//! - `mpris/`：系统媒体控制的规格笔记（实现走根级 `media_session.rs`，playwire）
//! - `sni/`：托盘 StatusNotifierItem（实现走根级 `tray.rs`，ksni feature）
//! - `x11/`：X11 专属能力（待补）
//! - `wayland/`：Wayland 专属能力（协议限制说明与 Portal 规划）
//!
//! ## 托盘：用 tray-icon + ksni feature，不自建
//!
//! 托盘跨平台实现在根级 `tray.rs`（`tray-icon` crate）；
//! Linux 后端经构建配置开启 `ksni` feature（见 `Cargo.toml`）——
//! 走原生 StatusNotifierItem（纯 D-Bus），**去掉默认的 GTK3 依赖**
//! （默认 feature `libappindicator` 会拖入 `libappindicator` + `muda/gtk3`）。
//! 菜单经 DBusMenu 由托盘宿主原生渲染。
//!
//! 因此本模块**不需要** SNI 实现——但保留 `sni/` 目录，
//! 以备将来 crate 无法满足需求时自建。
//!
//! ## TODO
//!
//! pub mod accent;     // GTK / KDE 配置

mod audio;
pub mod mpris;
mod native;
mod process;
mod shell;
mod single_instance;
pub mod sni;
pub mod wayland;
mod window_system;
pub mod x11;

use crate::native_library::NativeLibraryLocator;
use crate::window_system::WindowSystem;

/// 当前平台的窗口系统实现（Wayland 路线；未实现项按降级约定返回 `None` / `false`）。
pub(crate) fn window_system() -> &'static dyn WindowSystem {
    &self::window_system::LinuxWindowSystem
}

/// 当前平台的原生库定位实现。
pub(crate) fn native_library() -> &'static dyn NativeLibraryLocator {
    &native::LinuxNativeLibraryLocator
}

pub(crate) use audio::device_prefixes;
pub(crate) use process::rss_bytes;
pub(crate) use shell::open_url;
pub(crate) use single_instance::{ensure_single_instance, poll_activation_events};

/// Linux 请求客户端装饰（CSD）：自绘标题栏，避免合成器再画一套（Wayland 双标题栏）。
pub(crate) fn prefers_client_decorations() -> bool {
    true
}
