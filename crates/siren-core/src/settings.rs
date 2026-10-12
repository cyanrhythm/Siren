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
