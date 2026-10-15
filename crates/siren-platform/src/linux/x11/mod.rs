//! X11 专属能力。
//!
//! | 能力 | 实现 |
//! |---|---|
//! | 窗口显隐 / 几何 / 图标 | `XMapWindow` / `XMoveResizeWindow` / `_NET_WM_ICON`（并入 `window_system.rs`，**待补**） |
//! | 全球快捷键 | `XGrabKey` |
//! | 歌词点击穿透 | `XShape` 扩展清空输入区域 |
//!
//! ## 注意
//!
//! XWayland 会话下同样可用（Melodia 已验证）。
//!
//! ## 待补说明
//!
//! 本轮按 **Wayland 优先**路线落地：Wayland 会话的窗口操作由 GPUI 兜底
//! （见 `../window_system.rs` 的降级表），X11 的句柄层实现排在之后。
//! 实现时：`hwnd_from` 解析 `RawWindowHandle::Xcb` / `Xlib` 返回窗口 id，
//! `window_system.rs` 的对应方法转为真实调用（届时便捷函数自动生效，
//! 上层无需改动）。
//!
//! TODO: 实现（x11rb；图标另可经 GPUI `WindowOptions.icon`，X11 only）
