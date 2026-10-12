//! Linux：MPRIS 后端不需要窗口句柄。

use raw_window_handle::RawWindowHandle;

/// Linux 走 D-Bus（与显示协议无关），无需原生窗口句柄。
pub fn hwnd_from(_handle: Option<RawWindowHandle>) -> Option<u64> {
    None
}
