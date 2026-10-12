//! StatusNotifierItem（D-Bus 托盘）。
//!
//! ## 说明
//!
//! 若 `tray-icon` crate 能满足需求，**本模块可整体省略**。
//! 保留此目录用于：crate 不足时的自研实现，或需要精细控制时。
//!
//! Melodia 曾手写约 400 行 SNI 实现（含 DBusMenu 菜单），
//! 若需自研可参考，但**优先考虑 crate**。
//!
//! ## 为什么不用 XEmbed
//!
//! Melodia 的实测结论：
//! - Wayland 下经 `xembedsniproxy` 桥接，**收不到鼠标事件**
//! - XEmbed 渲染**不吃 alpha**（图标圆角外被填白）
//!
//! SNI 由托盘宿主直接回调 `Activate` / `ContextMenu`，点击可靠、支持透明图标。
//!
//! TODO: 视 tray-icon 评估结果决定是否实现
//! - StatusNotifierItem 对象导出
//! - DBusMenu 菜单布局
//! - 图标 PNG 编码
