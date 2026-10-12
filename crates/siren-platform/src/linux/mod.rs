//! Linux 平台实现。
//!
//! ## 会话类型分支
//!
//! X11 与 Wayland 的差异**不是** GUI 工具包能抹平的——
//! Wayland 协议本身不提供全局热键、窗口点击穿透、获取他人窗口句柄等能力。
//!
//! 因此按 `XDG_SESSION_TYPE` 分支，并明确降级：
//!
//! | 能力 | X11 | Wayland |
//! |---|---|---|
//! | 媒体控制（MPRIS） | ✅ | ✅（D-Bus，与显示协议无关） |
//! | 托盘（SNI） | ✅ | ✅（D-Bus，与显示协议无关） |
//! | 全局快捷键 | ✅ `XGrabKey` | ⚠️ 仅 XDG Portal，非所有 DE 支持 |
//! | 歌词点击穿透 | ✅ `XShape` | ❌ 无协议，降级 |
//! | 窗口装饰 | 交给 WM/CSD | 同 |
//!
//! ## 目录组织
//!
//! - `mpris/`：系统媒体控制（D-Bus，与显示协议无关）
//! - `sni/`：托盘 StatusNotifierItem（D-Bus，与显示协议无关）
//! - `x11/`：X11 专属能力
//! - `wayland/`：Wayland 专属能力（含 Portal）
//!
//! TODO: 补齐各模块
//!
//! pub mod media;      // MPRIS
//! pub mod accent;     // GTK / KDE 配置
//! pub mod session;    // XDG_SESSION_TYPE 探测

pub mod mpris;
pub mod native;
pub mod sni;
pub mod wayland;
pub mod x11;
