//! 单实例：命名互斥体判定；已有实例时唤起其窗口。
//!
//! 互斥体名建议用应用标识（如 `com.cyanrhythm.Siren`）。首个实例持有互斥体；
//! 后续实例 `CreateMutexW` 返回 `ERROR_ALREADY_EXISTS`，此时尝试把已有主窗口置前再退出。

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};

/// 主窗口标题（与 `siren-app` 的 `TitlebarOptions.title` 一致），用于唤起已有窗口。
const MAIN_WINDOW_TITLE: windows::core::PCWSTR = w!("Siren");

/// 保活互斥体句柄（进程退出时由系统回收）。
static MUTEX: AtomicIsize = AtomicIsize::new(0);

/// 若为唯一实例返回 `true`；否则尝试唤起已有窗口并返回 `false`。
pub fn ensure_single_instance(id: &str) -> bool {
    let name = HSTRING::from(id);
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr()));
        let last = GetLastError();
        match handle {
            Ok(h) if last != ERROR_ALREADY_EXISTS => {
                // 首个实例：保留句柄（进程存活期间一直持有）。
                MUTEX.store(h.0 as isize, Ordering::SeqCst);
                true
            }
            other => {
                if let Ok(h) = other {
                    let _ = CloseHandle(h);
                }
                focus_existing();
                false
            }
        }
    }
}

/// 把已有主窗口恢复到前台。
fn focus_existing() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, MAIN_WINDOW_TITLE) {
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}
