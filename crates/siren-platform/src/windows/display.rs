//! 主显示器可用区域与窗口移动。

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowRect, IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindowAsync,
    SystemParametersInfoW, SET_WINDOW_POS_FLAGS, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

fn raw_work_area() -> Option<RECT> {
    unsafe {
        // 已设置过会返回 Err，忽略即可
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut rect = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        return None;
    }
    Some(rect)
}

/// 主显示器工作区的**物理**坐标 `(x, y, w, h)`。
pub fn primary_work_area_physical() -> Option<(i32, i32, i32, i32)> {
    let rect = raw_work_area()?;
    Some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}

/// 所有显示器合成的虚拟屏幕（物理坐标）`(x, y, w, h)`。
///
/// 用于判断恢复的窗口位置是否仍落在可见范围内（显示器拔除/改分辨率后可能落到屏幕外）。
pub fn virtual_screen_physical() -> Option<(i32, i32, i32, i32)> {
    let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    (w > 0 && h > 0).then_some((x, y, w, h))
}

/// 主显示器工作区（排除任务栏）的逻辑坐标 `(x, y, w, h)`。
pub fn primary_work_area() -> Option<(f32, f32, f32, f32)> {
    let (x, y, w, h) = primary_work_area_physical()?;
    let dpi = unsafe { GetDpiForSystem() };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    Some((x as f32 / scale, y as f32 / scale, w as f32 / scale, h as f32 / scale))
}

/// 移动并调整窗口到物理坐标（不含 z-order / 激活）。
pub fn move_window(hwnd: u64, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        let flags = SET_WINDOW_POS_FLAGS(SWP_NOZORDER.0 | SWP_NOACTIVATE.0);
        let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
        let _ = SetWindowPos(handle, None, x, y, width, height, flags);
    }
}

/// 显示窗口并置前（用于隐藏创建后展示）。
pub fn show_window(hwnd: u64) {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    unsafe {
        let _ = ShowWindowAsync(handle, SW_SHOW);
        let _ = SetForegroundWindow(handle);
    }
}

/// 恢复（若最小化则还原）窗口并置前。
pub fn restore_window(hwnd: u64) {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    unsafe {
        let _ = ShowWindowAsync(handle, SW_RESTORE);
        let _ = SetForegroundWindow(handle);
    }
}

/// 隐藏窗口（最小化到托盘）。
pub fn hide_window(hwnd: u64) {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    unsafe {
        let _ = ShowWindowAsync(handle, SW_HIDE);
    }
}

/// 窗口当前是否可见。
pub fn is_window_visible(hwnd: u64) -> bool {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    unsafe { IsWindowVisible(handle).as_bool() }
}

/// 窗口是否处于最小化（最小化时 `GetWindowRect` 会返回屏幕外坐标，不可用于记忆位置）。
pub fn is_window_minimized(hwnd: u64) -> bool {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    unsafe { IsIconic(handle).as_bool() }
}

/// 切换窗口显隐；返回切换后是否可见。
pub fn toggle_window(hwnd: u64) -> bool {
    if is_window_visible(hwnd) {
        hide_window(hwnd);
        false
    } else {
        restore_window(hwnd);
        true
    }
}

/// 窗口的物理矩形 `(x, y, w, h)`。
pub fn window_physical_rect(hwnd: u64) -> Option<(i32, i32, i32, i32)> {
    let handle = HWND(hwnd as usize as *mut core::ffi::c_void);
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(handle, &mut rect) }.is_err() {
        return None;
    }
    Some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}
