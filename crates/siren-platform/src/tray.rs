//! 托盘与窗口显隐的平台无关接口。
//!
//! 具体实现在各平台子模块（Windows：`windows::tray`），上层只依赖这里的类型。

/// 托盘交互事件（平台无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// 左键单击托盘图标：切换主窗口显隐。
    ToggleWindow,
    /// 菜单「退出」。
    Quit,
}
