//! 用户设置持久化（`config_dir/settings.json`）。
//!
//! 与 [`crate::auth`] 的会话文件同目录。写入失败静默忽略，不阻断业务流程。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::AudioQuality;

/// 设置文件路径：`config_dir/settings.json`。
fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren").map(|dirs| dirs.config_dir().join("settings.json"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Settings {
    /// 音质档位代码（如 `exhigh`）。
    quality: Option<String>,
    /// 是否启用系统媒体控制（SMTC / MPRIS），默认开启。
    media_keys: Option<bool>,
    /// 正在播放卡片顺序（代码列表，见 [`DEFAULT_CARDS`]）。
    cards: Option<Vec<String>>,
    /// 右侧栏宽度（像素）。
    np_width: Option<f32>,
}

/// 右侧栏宽度范围与默认值。
pub const NP_WIDTH_MIN: f32 = 300.0;
pub const NP_WIDTH_MAX: f32 = 800.0;
pub const NP_WIDTH_DEFAULT: f32 = 384.0;

/// 正在播放面板卡片的默认顺序。
pub const DEFAULT_CARDS: [&str; 7] = [
    "lyrics",
    "comments",
    "song_detail",
    "music_memory",
    "about_artist",
    "artist_albums",
    "similar_artists",
];

/// 读音质档位（缺省默认）。
pub fn load_card_layout() -> Vec<String> {
    let cards = read().cards.unwrap_or_default();
    if cards.is_empty() {
        DEFAULT_CARDS.iter().map(|s| s.to_string()).collect()
    } else {
        cards
    }
}

/// 保存正在播放卡片顺序。
pub fn save_card_layout(cards: &[String]) {
    let mut settings = read();
    settings.cards = Some(cards.to_vec());
    write(&settings);
}

/// 读取右侧栏宽度（夹在 [`NP_WIDTH_MIN`]..[`NP_WIDTH_MAX`]，缺省 [`NP_WIDTH_DEFAULT`]）。
pub fn load_np_width() -> f32 {
    read()
        .np_width
        .unwrap_or(NP_WIDTH_DEFAULT)
        .clamp(NP_WIDTH_MIN, NP_WIDTH_MAX)
}

/// 保存右侧栏宽度。
pub fn save_np_width(width: f32) {
    let mut settings = read();
    settings.np_width = Some(width.clamp(NP_WIDTH_MIN, NP_WIDTH_MAX));
    write(&settings);
}

fn read() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, serde_json::to_string_pretty(settings).unwrap_or_default());
}

/// 读取已保存的音质档位（缺省为默认档）。
pub fn load_quality() -> AudioQuality {
    read()
        .quality
        .map(|code| AudioQuality::from_code(&code))
        .unwrap_or_default()
}

/// 保存音质档位。
pub fn save_quality(quality: AudioQuality) {
    let mut settings = read();
    settings.quality = Some(quality.code().to_string());
    write(&settings);
}

/// 是否启用系统媒体控制（默认开启）。
pub fn load_media_keys() -> bool {
    read().media_keys.unwrap_or(true)
}

/// 保存系统媒体控制开关。
pub fn save_media_keys(enabled: bool) {
    let mut settings = read();
    settings.media_keys = Some(enabled);
    write(&settings);
}
