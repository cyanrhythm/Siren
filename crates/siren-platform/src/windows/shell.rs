//! Windows：系统 Shell 集成。

/// 用系统默认程序打开 URL。
///
/// `start` 的首个带引号参数会被当作窗口标题，故传入空标题占位。
pub(crate) fn open_url(url: &str) {
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
}
