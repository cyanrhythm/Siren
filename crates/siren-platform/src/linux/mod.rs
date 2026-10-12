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
//! | 全局快捷键 | ⚠️ 仅 XDG Portal，非所有 DE 支持 | ✅ `XGrabKey` |
//! | 歌词点击穿透 | ❌ 无协议，降级 | ✅ `XShape` |
//! | 窗口装饰 | 交给合成器 | 同 |
//!
//! ## 目录组织
//!
//! - `mpris/`：系统媒体控制（D-Bus，与显示协议无关）
//! - `sni/`：托盘 StatusNotifierItem（D-Bus，与显示协议无关）
//! - `x11/`：X11 专属能力
//! - `wayland/`：Wayland 专属能力（含 Portal）
//!
//! ## 托盘：用 tray-icon + ksni feature，不自建
//!
//! 托盘**不在此处实现**，直接使用 `tray-icon` crate，并在构建配置上开启
//! `ksni` feature：
//!
//! ```toml
//! tray-icon = { version = "0.26", default-features = false, features = ["ksni"] }
//! ```
//!
//! 这样走原生 StatusNotifierItem（纯 D-Bus），**去掉默认的 GTK3 依赖**
//! （默认 feature `libappindicator` 会拖入 `libappindicator` + `muda/gtk3`）。
//! 菜单经 DBusMenu 由托盘宿主原生渲染。
//!
//! 因此本模块**不需要** SNI 实现——但保留 `sni/` 目录，
//! 以备将来 crate 无法满足需求时自建。
//!
//! ## TODO
//!
//! pub mod media;      // MPRIS
//! pub mod accent;     // GTK / KDE 配置
//! pub mod session;    // XDG_SESSION_TYPE 探测

pub mod media;
pub mod mpris;
pub mod native;
pub mod sni;
pub mod wayland;
pub mod x11;
