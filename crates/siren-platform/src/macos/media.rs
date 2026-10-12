//! macOS：Now Playing 不需要窗口句柄。

use raw_window_handle::RawWindowHandle;

/// macOS 走 `MPNowPlayingInfoCenter`，无需原生窗口句柄。
pub fn hwnd_from(_handle: Option<RawWindowHandle>) -> Option<u64> {
    None
}
