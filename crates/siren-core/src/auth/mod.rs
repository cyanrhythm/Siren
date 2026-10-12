//! 认证：登录、凭据持久化、用户信息同步。
//!
//! 对应 Melodia 的 `core/auth/`。当前落地 Cookie 登录与会话持久化；
//! 扫码登录待后续补充。

use std::path::PathBuf;

use crate::network::api;
use crate::network::client::NeteaseClient;

/// 会话文件路径：`config_dir/session.json`。
///
/// 使用 `directories` crate（跨平台，无 `#[cfg]`）：
/// Windows `%APPDATA%\Siren`、Linux `~/.config/siren`、macOS `~/Library/Application Support/com.cyanrhythm.Siren`。
fn session_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren").map(|dirs| dirs.config_dir().join("session.json"))
}

/// 读取已保存的 Cookie。
pub fn load_cookie() -> Option<String> {
    let path = session_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("cookie")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 保存 Cookie（写入失败静默忽略，不阻断登录流程）。
pub fn save_cookie(cookie: &str) {
    let Some(path) = session_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::json!({ "cookie": cookie });
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap_or_default());
}

/// 清除已保存的 Cookie。
pub fn clear_cookie() {
    if let Some(path) = session_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// 归一化用户输入：接受完整 Cookie 串或裸 `MUSIC_U` 值。
pub fn normalize_cookie(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if s.contains('=') {
        Some(s.to_string())
    } else {
        Some(format!("MUSIC_U={s}"))
    }
}

/// 拉取昵称以校验登录态；未登录或无效返回 `None`。
pub async fn fetch_nickname(client: &NeteaseClient) -> Option<String> {
    api::account(client).await.ok().and_then(|v| {
        v.get("profile")
            .and_then(|p| p.get("nickname"))
            .and_then(|n| n.as_str())
            .map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_cookie;

    #[test]
    fn normalize_accepts_full_cookie() {
        assert_eq!(
            normalize_cookie("MUSIC_U=abc; __csrf=xyz").as_deref(),
            Some("MUSIC_U=abc; __csrf=xyz")
        );
    }

    #[test]
    fn normalize_wraps_bare_music_u() {
        assert_eq!(normalize_cookie("abc123").as_deref(), Some("MUSIC_U=abc123"));
        assert_eq!(normalize_cookie("  "), None);
    }
}
