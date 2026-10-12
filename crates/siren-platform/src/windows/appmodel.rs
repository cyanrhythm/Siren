//! Windows 进程级 AppUserModelID 注册。
//!
//! 系统媒体控制（SMTC）据 AppUserModelID 解析应用名/图标；不注册时媒体浮层显示
//! “未知应用”。这里在进程上显式注册，与 `siren-platform` 传给 playwire 的
//! `app_media_id` 保持一致。完整名称仍需安装器在开始菜单快捷方式上注册同一 ID。

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

/// 注册当前进程的 AppUserModelID。
pub fn set_app_user_model_id(id: &str) {
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr()));
    }
}
