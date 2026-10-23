//! 窗口图标：从 exe 内嵌资源加载并设置到窗口（任务栏 / Alt-Tab / 标题）。
//!
//! 资源由 `crates/siren-app/build.rs`（`winresource`）写入，id 为 1。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    LoadIconW, SendMessageW, SetClassLongPtrW, GCLP_HICON, GCLP_HICONSM, ICON_BIG, ICON_SMALL, WM_SETICON,
};

/// 内嵌应用图标的资源 id（`winresource::set_icon` 生成 `1 ICON "..."`）。
const APP_ICON_RESOURCE: usize = 1;

/// 把内嵌应用图标设为窗口图标；成功返回 `true`。
pub fn set_window_icon(hwnd: u64) -> bool {
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return false;
        };
        let hinst = HINSTANCE(hmodule.0);
        let Ok(icon) = LoadIconW(Some(hinst), PCWSTR(APP_ICON_RESOURCE as *const u16)) else {
            return false;
        };
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        // 大/小图标分别用于 Alt-Tab/任务栏 与 标题栏
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(icon.0 as isize)),
        );
        SetClassLongPtrW(hwnd, GCLP_HICON, icon.0 as isize);
        SetClassLongPtrW(hwnd, GCLP_HICONSM, icon.0 as isize);
        true
    }
}
