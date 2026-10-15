//! 窗口系统集成：窗口显隐 / 几何 / 图标与进程窗口标识。
//!
//! 平台差异能力 → **根级接口 + 平台目录实现**：
//! 实现在 `windows/window_system.rs`（Win32）、`linux/window_system.rs`（Wayland
//! 路线：协议不可及项降级）与 `macos/window_system.rs`（待补，当前降级为 no-op）。
//!
//! 便捷函数**直接接收窗口**（`HasWindowHandle`），原生句柄的解析由平台实现完成
//! （[`WindowSystem::hwnd_from`]）；句柄不可得时（如 Wayland 无窗口标识）按降级约定
//! 返回 `None` / `false` / no-op，绝不 panic。
//!
//! ## 降级契约（上层据此选择 GPUI 兜底）
//!
//! 句柄层做不到的窗口操作，**由平台明确报告「未处理」**，上层改用 GPUI 自带的
//! 窗口 API（`Window::minimize_window` / `activate_window` / `window_bounds`）兜底：
//!
//! | 便捷函数 | 返回值 | Wayland 上的兜底（siren-app） |
//! |---|---|---|
//! | [`hide_window`] | 是否已隐藏 | 最小化（`minimize_window`），进程留在托盘 |
//! | [`toggle_window`] | 是否已接管切换 | 前台则最小化、否则激活 |
//! | [`move_window`] | 是否已移动 | 仅恢复尺寸（`Window::resize`），位置由合成器决定 |
//! | [`window_physical_rect`] | `None` | 退化为 GPUI 逻辑坐标（`window_bounds`） |
//!
//! 原因：Wayland 的 xdg-shell 不提供显隐 / 绝对定位 / 几何查询协议；任何工具包
//! （含 GPUI）都只能在**持有图形连接**的层执行这些操作——即应用层的 GPUI API。

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 窗口系统契约。
pub trait WindowSystem: Send + Sync {
    /// 注册进程级 AppUserModelID（Windows 媒体浮层显示应用名；其余平台忽略）。
    fn set_app_user_model_id(&self, id: &str);

    /// 从 `raw-window-handle` 解析平台窗口句柄。
    ///
    /// 仅当句柄层**确有**窗口操作实现时才返回值：Windows 返回 `HWND`；
    /// Linux（Wayland / X11 均未实现句柄层操作）与 macOS 恒 `None`——
    /// 上层据此走 GPUI 兜底路径。
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

    /// 移动并调整窗口到物理坐标；返回是否已执行（平台未实现时 `false`）。
    fn move_window(&self, hwnd: u64, x: i32, y: i32, width: i32, height: i32) -> bool;

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

    /// 切换窗口显隐；返回是否已接管（平台未实现时 `false`，由上层兜底）。
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

/// 平台层持有的原生窗口引用：句柄封装在内部，供上层把窗口操作**延迟到 App 借用之外**执行。
///
/// 上层在窗口上下文中用 [`capture_window`] 取得它（无副作用、不接触原生句柄），
/// 随后在合适时机（异步任务里、`window_handle.update` 返回之后）调用其方法。
///
/// 背景：`SetWindowPos` / `SetForegroundWindow` 会**同步派发**窗口消息；若在 GPUI 的
/// App 借用期间调用，会重入窗口过程并再次借用 App → `RefCell already borrowed` panic。
#[derive(Clone, Copy)]
pub struct CapturedWindow {
    hwnd: u64,
}

impl CapturedWindow {
    /// 移动到物理坐标；返回是否已执行（`false` 时上层用 GPUI 兜底）。
    pub fn move_to(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        window_system().move_window(self.hwnd, x, y, width, height)
    }

    /// 设置窗口图标；成功返回 `true`。
    pub fn set_icon(&self) -> bool {
        window_system().set_window_icon(self.hwnd)
    }

    /// 显示窗口并置前。
    pub fn show(&self) {
        window_system().show_window(self.hwnd);
    }

    /// 切换窗口显隐；返回是否已执行（`false` 时上层用 GPUI 兜底）。
    pub fn toggle(&self) -> bool {
        window_system().toggle_window(self.hwnd)
    }
}

/// 在窗口上下文中捕获原生窗口（无副作用）；句柄不可得（非 Windows）返回 `None`。
pub fn capture_window<W: HasWindowHandle + ?Sized>(window: &W) -> Option<CapturedWindow> {
    window_hwnd(window).map(|hwnd| CapturedWindow { hwnd })
}

/// 窗口的物理矩形 `(x,y,w,h)`（句柄不可得 / 非 Windows 返回 None）。
///
/// `None` 时上层退化为 GPUI 逻辑坐标（`Window::window_bounds`），见本模块文档的降级契约。
pub fn window_physical_rect<W: HasWindowHandle + ?Sized>(window: &W) -> Option<(i32, i32, i32, i32)> {
    let hwnd = window_hwnd(window)?;
    window_system().window_physical_rect(hwnd)
}

/// 将窗口移动到物理坐标；返回是否已执行（`false` 时上层应用 GPUI 兜底）。
pub fn move_window<W: HasWindowHandle + ?Sized>(window: &W, x: i32, y: i32, width: i32, height: i32) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().move_window(hwnd, x, y, width, height)
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

/// 隐藏窗口（最小化到托盘）；返回是否已生效。
///
/// `false`（句柄不可得 / 平台无隐藏能力，如 Wayland）时上层降级为
/// `Window::minimize_window`，进程保持驻留托盘。
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

/// 切换窗口显隐；返回是否已由平台接管。
///
/// `false`（如 Wayland 无显隐协议）时上层改用 GPUI 兜底：窗口在台前则
/// `Window::minimize_window`，否则 `Window::activate_window`。
pub fn toggle_window<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().toggle_window(hwnd)
}

/// 设置窗口图标，返回是否成功（句柄不可得 / 非 Windows 恒 `false`）。
pub fn set_window_icon<W: HasWindowHandle + ?Sized>(window: &W) -> bool {
    let Some(hwnd) = window_hwnd(window) else { return false };
    window_system().set_window_icon(hwnd)
}
