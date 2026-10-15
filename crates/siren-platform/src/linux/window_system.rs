//! Linux：窗口系统实现（**Wayland 优先**；X11 待补）。
//!
//! ## Wayland 路线：协议不可及者彻底降级
//!
//! Wayland 的 xdg-shell **协议本身**不提供下列能力（非实现缺失，任何工具包同理）：
//!
//! | 能力 | 协议现状 | 本实现 | 上层（siren-app）兜底 |
//! |---|---|---|---|
//! | 显隐 / 置前 | 无 map / unmap / raise 请求（仅 `set_minimized`） | 降级 `false` | `Window::minimize_window` / `activate_window` |
//! | 绝对定位 / 几何查询 | 无请求（位置由合成器裁决） | 降级 `None` / `false` | `Window::resize`（仅尺寸）+ `window_bounds`（逻辑坐标） |
//! | 工作区 / 虚拟屏幕 | 无标准协议 | 降级 `None` | 不居中（合成器自会放置首次窗口） |
//! | 窗口图标 | 无逐窗口图标（来自 .desktop ↔ app_id 匹配） | 降级 `false` | 打包提供 `.desktop`（待做） |
//!
//! 降级操作只有**图形连接持有者**（GPUI）能执行：`wl_surface` 的角色代理
//! （`xdg_toplevel`）不外露，句柄层拿不到（见 `wayland/mod.rs` 与
//! 根级 `window_system.rs` 的降级契约）。
//!
//! ## X11（含 XWayland）
//!
//! **待补**（x11rb）：显隐（`XMapWindow` / `XUnmapWindow`）、几何
//! （`XMoveResizeWindow` + `_NET_WORKAREA`）、图标（`_NET_WM_ICON`）。
//! 届时 [`WindowSystem::hwnd_from`] 解析 `Xcb` / `Xlib` 句柄并返回窗口 id，
//! 上层便捷函数即自动走句柄层——在此之前与 Wayland 同样降级，绝不谎报成功。

use raw_window_handle::RawWindowHandle;

use crate::window_system::WindowSystem;

/// Linux 窗口系统实现（句柄层能力未实现：全部按降级约定返回 `None` / `false`）。
pub struct LinuxWindowSystem;

impl WindowSystem for LinuxWindowSystem {
    fn set_app_user_model_id(&self, _id: &str) {
        // Wayland 应用标识经 `WindowOptions::app_id`（GPUI 侧 set_app_id）设置；
        // X11 的 WM_CLASS 同源。
    }

    fn hwnd_from(&self, _handle: Option<RawWindowHandle>) -> Option<u64> {
        // X11 实现落地后：解析 Xcb / Xlib 句柄返回窗口 id。
        None
    }

    fn primary_work_area(&self) -> Option<(f32, f32, f32, f32)> {
        None
    }

    fn primary_work_area_physical(&self) -> Option<(i32, i32, i32, i32)> {
        None
    }

    fn window_physical_rect(&self, _hwnd: u64) -> Option<(i32, i32, i32, i32)> {
        None
    }

    fn move_window(&self, _hwnd: u64, _x: i32, _y: i32, _width: i32, _height: i32) -> bool {
        false
    }

    fn show_window(&self, _hwnd: u64) {}

    fn restore_window(&self, _hwnd: u64) {}

    fn hide_window(&self, _hwnd: u64) {}

    fn is_window_visible(&self, _hwnd: u64) -> bool {
        false
    }

    fn is_window_minimized(&self, _hwnd: u64) -> bool {
        false
    }

    fn toggle_window(&self, _hwnd: u64) -> bool {
        false
    }

    fn set_window_icon(&self, _hwnd: u64) -> bool {
        false
    }
}
