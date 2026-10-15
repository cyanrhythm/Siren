//! macOS：窗口系统实现（当前降级为 no-op / `None`）。
//!
//! 待补：AppKit（窗口显隐 / 位置 / 图标）与 `NSWindow.ignoresMouseEvents` 等；
//! 在此之前全部降级（见 `docs/02-平台能力规格.md`）。

use raw_window_handle::RawWindowHandle;

use crate::window_system::WindowSystem;

/// macOS 窗口系统实现（降级占位）。
pub struct MacosWindowSystem;

impl WindowSystem for MacosWindowSystem {
    fn set_app_user_model_id(&self, _id: &str) {}

    fn hwnd_from(&self, _handle: Option<RawWindowHandle>) -> Option<u64> {
        // macOS 走 `MPNowPlayingInfoCenter`，不需要原生窗口句柄（设计使然）。
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

    fn move_window(&self, _hwnd: u64, _x: i32, _y: i32, _width: i32, _height: i32) {}

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
