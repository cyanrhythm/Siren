//! 系统 Shell 集成：用系统默认程序打开 URL 等。
//!
//! 平台差异能力 → 根级统一函数契约 + 平台目录实现
//! （Windows `cmd /C start`；Linux `xdg-open`；macOS `open`）。

/// 用系统默认程序打开 URL（如网页登录）。
pub fn open_url(url: &str) {
    crate::imp::open_url(url);
}
