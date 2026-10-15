//! Linux：系统 Shell 集成。

/// 用系统默认程序打开 URL（`xdg-open`）。
pub(crate) fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
