//! Windows：窗口系统实现（Win32）。
//!
//! 契约见 [`crate::window_system::WindowSystem`]；系统调用在
//! `display` / `icon` / `appmodel` 子模块中。

use raw_window_handle::RawWindowHandle;

use crate::window_system::WindowSystem;

use super::{appmodel, display, icon};

/// Windows 窗口系统实现。
pub struct WindowsWindowSystem;

impl WindowSystem for WindowsWindowSystem {
    fn set_app_user_model_id(&self, id: &str) {
        appmodel::set_app_user_model_id(id);
    }

    fn hwnd_from(&self, handle: Option<RawWindowHandle>) -> Option<u64> {
        match handle? {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get() as u64),
            _ => None,
        }
    }

    fn primary_work_area(&self) -> Option<(f32, f32, f32, f32)> {
        display::primary_work_area()
    }

    fn primary_work_area_physical(&self) -> Option<(i32, i32, i32, i32)> {
        display::primary_work_area_physical()
    }

    fn virtual_screen_physical(&self) -> Option<(i32, i32, i32, i32)> {
        display::virtual_screen_physical()
    }

    fn window_physical_rect(&self, hwnd: u64) -> Option<(i32, i32, i32, i32)> {
        display::window_physical_rect(hwnd)
    }

    fn move_window(&self, hwnd: u64, x: i32, y: i32, width: i32, height: i32) -> bool {
        display::move_window(hwnd, x, y, width, height);
        true
    }

    fn show_window(&self, hwnd: u64) {
        display::show_window(hwnd);
    }

    fn restore_window(&self, hwnd: u64) {
        display::restore_window(hwnd);
    }

    fn hide_window(&self, hwnd: u64) {
        display::hide_window(hwnd);
    }

    fn is_window_visible(&self, hwnd: u64) -> bool {
        display::is_window_visible(hwnd)
    }

    fn is_window_minimized(&self, hwnd: u64) -> bool {
        display::is_window_minimized(hwnd)
    }

    fn toggle_window(&self, hwnd: u64) -> bool {
        display::toggle_window(hwnd);
        true
    }

    fn set_window_icon(&self, hwnd: u64) -> bool {
        icon::set_window_icon(hwnd)
    }
}
