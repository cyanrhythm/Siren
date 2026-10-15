//! Wayland 专属能力。
//!
//! ## 协议限制（非实现问题）
//!
//! Wayland 出于安全设计，**不提供**以下能力——GTK / Qt / GPUI 均受同样约束：
//!
//! - 窗口显隐 / 绝对定位 / 几何查询（xdg-shell 无对应请求）
//! - 逐窗口图标（图标来自 `.desktop` ↔ `app_id` 匹配）
//! - 全局热键（仅可通过 XDG Desktop Portal，需用户授权）
//! - 窗口点击穿透（无对外协议）
//! - 获取他人窗口句柄
//!
//! ## 本项目的落地方式
//!
//! | 受限能力 | 落地 |
//! |---|---|
//! | 窗口显隐 / 置前 | 句柄层降级（`window_system.rs`）；GPUI `Window::minimize_window` / `activate_window` 兜底 |
//! | 窗口几何 | 仅恢复尺寸（GPUI `resize`）；位置由合成器决定 |
//! | 单实例唤起 | D-Bus `org.freedesktop.Application.Activate` → GPUI 激活（见 `single_instance.rs` 的合成器拒答说明） |
//! | 全局快捷键 | XDG Portal（`org.freedesktop.portal.GlobalShortcuts`），非所有 DE 实现——暂不接入，以 MPRIS 媒体键为主 |
//!
//! ## 降级策略
//!
//! 不可实现的能力应**明确降级并在界面/文档说明**，而非静默失败。
//!
//! TODO（Portal 方向）：
//! - `GlobalShortcuts` 门户接入（用户授权流程、快捷键持久化由门户管理）
