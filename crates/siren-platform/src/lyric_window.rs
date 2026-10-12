//! 桌面歌词窗口的行为控制。
//!
//! ## ⚠️ 平台差异最大的一项
//!
//! | 场景 | 方案 | 可行性 |
//! |---|---|---|
//! | X11 / XWayland | `XShape` 扩展清空输入区域 | ✅ |
//! | Wayland 原生 | **无对外协议** | ❌ 降级为置顶不穿透 |
//! | Windows | `WS_EX_TRANSPARENT \| WS_EX_LAYERED` | ✅ |
//! | macOS | `NSWindow.ignoresMouseEvents` | ✅ |
//!
//! ## 关键事实
//!
//! Wayland 出于安全设计**不允许**客户端设置输入穿透。
//! 任何 GUI 工具包（含 GPUI）都受此限制——这不是实现问题，是协议问题。
//!
//! ## 降级策略
//!
//! Wayland 下窗口置顶显示但可点击，并在设置页说明该限制。
//!
//! TODO: 定义 trait
//! - set_click_through(window, enabled)
