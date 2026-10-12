//! Windows：SMTC 后端所需的窗口句柄解析。

use raw_window_handle::RawWindowHandle;

/// 从 `raw-window-handle` 解析 SMTC 所需的 HWND。
pub fn hwnd_from(handle: Option<RawWindowHandle>) -> Option<u64> {
    match handle? {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as u64),
        _ => None,
    }
}
