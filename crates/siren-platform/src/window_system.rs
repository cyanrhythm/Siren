//! 窗口系统集成：窗口显隐 / 几何 / 图标与进程窗口标识。
//!
//! 平台差异能力 → **根级接口 + 平台目录实现**：
//! 实现在 `windows/window_system.rs`（Win32）、`linux/window_system.rs` 与
//! `macos/window_system.rs`（待补，当前降级为 no-op）。
//!
//! 便捷函数**直接接收窗口**（`HasWindowHandle`），原生句柄的解析由平台实现完成
//! （[`WindowSystem::hwnd_from`]）；句柄不可得时（非 Windows）按降级约定返回
//! `None` / `false` / no-op，绝不 panic，上层据返回值决定降级行为。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 窗口系统契约。
pub trait WindowSystem: Send + Sync {
    /// 注册进程级 AppUserModelID（Windows 媒体浮层显示应用名；其余平台忽略）。
    fn set_app_user_model_id(&self, id: &str);

    /// 从 `raw-window-handle` 解析平台窗口句柄（非 Windows 恒 `None`）。
    fn hwnd_from(&self, handle: Option<RawWindowHandle>) -> Option<u64>;

    /// 主显示器工作区（逻辑像素 `x,y,w,h`；非 Windows 返回 `None`）。
    fn primary_work_area(&self) -> Option<(f32, f32, f32, f32)>;

    /// 主显示器工作区（物理像素；非 Windows 返回 `None`）。
    fn primary_work_area_physical(&self) -> Option<(i32, i32, i32, i32)>;

    /// 所有显示器合成的虚拟屏幕（物理像素 `x,y,w,h`）；无实现时返回 `None`。
    fn virtual_screen_physical(&self) -> Option<(i32, i32, i32, i32)> {
        None
    }

    /// 窗口的物理矩形 `(x,y,w,h)`（非 Windows 返回 `None`）。
    fn window_physical_rect(&self, hwnd: u64) -> Option<(i32, i32, i32, i32)>;

    /// 移动并调整窗口到物理坐标（非 Windows 无操作）。
    fn move_window(&self, hwnd: u64, x: i32, y: i32, width: i32, height: i32);

    /// 显示窗口并置前（非 Windows 无操作）。
    fn show_window(&self, hwnd: u64);

    /// 恢复（最小化还原）并置前（非 Windows 无操作）。
    fn restore_window(&self, hwnd: u64);

    /// 隐藏窗口（最小化到托盘；非 Windows 无操作）。
    fn hide_window(&self, hwnd: u64);

    /// 窗口当前是否可见（非 Windows 恒 `false`）。
    fn is_window_visible(&self, hwnd: u64) -> bool;

    /// 窗口是否处于最小化（非 Windows 恒 `false`）。
    fn is_window_minimized(&self, hwnd: u64) -> bool;

    /// 切换窗口显隐；返回切换后是否可见（非 Windows 恒 `false`）。
    fn toggle_window(&self, hwnd: u64) -> bool;

    /// 设置窗口图标；成功返回 `true`（非 Windows 恒 `false`）。
    fn set_window_icon(&self, hwnd: u64) -> bool;
}

/// 当前平台的窗口系统实现（低层入口；上层一般用下方便捷函数）。
pub fn window_system() -> &'static dyn WindowSystem {
    crate::imp::window_system()
}

// ── 便捷函数（上层唯一入口）────────────────────────────────────────
//
// 直接接收窗口（`HasWindowHandle`）；句柄解析经平台实现
// （[`WindowSystem::hwnd_from`]）完成，上层不接触原生句柄。

/// 注册进程级 AppUserModelID（Windows 媒体浮层显示应用名；其余平台无操作）。
pub fn set_app_user_model_id(id: &str) {
    window_system().set_app_user_model_id(id);
}

/// 从窗口解析原生句柄。
///
/// **低层接口**：仅用于把原生句柄交给第三方 API（如 playwire 的 SMTC 配置）；
/// 常规窗口操作请使用下方的便捷函数（直接传窗口）。非 Windows 恒 `None`。
pub fn window_hwnd<W: HasWindowHandle + ?Sized>(window: &W) -> Option<u64> {
    let handle = window.window_handle().ok().map(|handle| handle.as_raw());
    window_system().hwnd_from(handle)
}

/// 主显示器工作区（逻辑像素 `x,y,w,h`；非 Windows 返回 None）。
pub fn primary_work_area() -> Option<(f32, f32, f32, f32)> {
    window_system().primary_work_area()
}

/// 主显示器工作区（物理像素；非 Windows 返回 None）。
pub fn primary_work_area_physical() -> Option<(i32, i32, i32, i32)> {
    window_system().primary_work_area_physical()
}

/// 所有显示器合成的虚拟屏幕（物理像素 `x,y,w,h`；无实现时返回 None）。
pub fn virtual_screen_physical() -> Option<(i32, i32, i32, i32)> {
    window_system().virtual_screen_physical()
}

/// 窗口的物理矩形 `(x,y,w,h)`（句柄不可得 / 非 Windows 返回 None）。
pub fn window_physical_rect<W: HasWindowHandle + ?Sized>(window: &W) -> Option<(i32, i32, i32, i32)> {
    let hwnd = window_hwnd(window)?;
    window_system().window_physical_rect(hwnd)
}

/// 将窗口移动到物理坐标（句柄不可得 / 非 Windows 无操作）。
pub fn move_window<W: HasWindowHandle + ?Sized>(window: &W, x: i32, y: i32, width: i32, height: i32) {
    let Some(hwnd) = window_hwnd(window) else { return };
    window_system().move_window(hwnd, x, y, width, height);
}

/// 显示窗口并置前（句柄不可得 / 非 Windows 无操作）。
pub fn show_window<W: HasWindowHandle + ?Sized>(window: &W) {
    let Some(hwnd) = window_hwnd(window) else { return };
    window_system().show_window(hwnd);
}

/// 恢复（最小化还原）并置前（句柄不可得 / 非 Windows 无操作）。
pub fn restore_window<W: HasWindowHandle + ?Sized>(window: &W) {
    let Some(hwnd) = window_hwnd(window) else { return };
    window_system().restore_window(hwnd);
}

/// 隐藏窗口（最小化到托盘）；返回是否已生效（句柄不可得 / 平台未实现时 `false`）。
pub fn hide_window<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().hide_window(hwnd);
    true
}

/// 窗口是否可见（句柄不可得 / 非 Windows 恒 `false`）。
pub fn is_window_visible<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().is_window_visible(hwnd)
}

/// 窗口是否最小化（句柄不可得 / 非 Windows 恒 `false`）。
pub fn is_window_minimized<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().is_window_minimized(hwnd)
}

/// 切换窗口显隐，返回切换后是否可见（句柄不可得 / 非 Windows 恒 `false`）。
pub fn toggle_window<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().toggle_window(hwnd)
}

/// 设置窗口图标，返回是否成功（句柄不可得 / 非 Windows 恒 `false`）。
pub fn set_window_icon<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().set_window_icon(hwnd)
}
