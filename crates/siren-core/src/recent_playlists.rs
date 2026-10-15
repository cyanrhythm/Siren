//! 本地「最近播放歌单」记录。
//!
//! 服务端不记录本客户端的歌单播放，故本地维护（对齐 Melodia `PlaybackPreferences.LocalRecentPlaylist`）：
//! 同一歌单只留最新一条，按播放时间倒序保留最近 [`MAX_RECENT_PLAYLISTS`] 个。
//!
//! 存储于 `config_dir/local_recent_playlists.json`；写入失败静默忽略。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 本地最近播放的一条歌单。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRecentPlaylist {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub cover_url: String,
    /// 播放时间（Unix 毫秒）。
    pub play_time: i64,
}

/// 本地保留的最大条数。
pub const MAX_RECENT_PLAYLISTS: usize = 20;

fn store_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren")
        .map(|dirs| dirs.config_dir().join("local_recent_playlists.json"))
}

/// 当前 Unix 毫秒时间戳。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 毫秒时间戳 → 本地时钟 `HH:MM`（所在日期已由分区头承担，故仅到分钟）；无效返回空串。
pub fn fmt_clock_time(ms: i64) -> String {
    use chrono::{Local, TimeZone};
    if ms <= 0 {
        return String::new();
    }
    match Local.timestamp_millis_opt(ms).single() {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => String::new(),
    }
}

/// 读取本地最近播放歌单（新的在前）。
pub fn load_recent_playlists() -> Vec<LocalRecentPlaylist> {
    let Some(path) = store_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 记录一次歌单播放：去重（同 id 只留最新）、倒序、截断。
pub fn record_recent_playlist(item: LocalRecentPlaylist) {
    let mut items = load_recent_playlists();
    items.retain(|existing| existing.id != item.id);
    items.insert(0, item);
    items.truncate(MAX_RECENT_PLAYLISTS);
    let Some(path) = store_path() else {
        return;
    };
    crate::persist::write_atomic(&path, &serde_json::to_string(&items).unwrap_or_default());
}
