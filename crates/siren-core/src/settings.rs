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
}

/// 读取已保存的音质档位（缺省为默认档）。
pub fn load_quality() -> AudioQuality {
    let Some(path) = settings_path() else {
        return AudioQuality::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return AudioQuality::default();
    };
    let Ok(settings) = serde_json::from_str::<Settings>(&text) else {
        return AudioQuality::default();
    };
    settings
        .quality
        .map(|code| AudioQuality::from_code(&code))
        .unwrap_or_default()
}

/// 保存音质档位。
pub fn save_quality(quality: AudioQuality) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let settings = Settings {
        quality: Some(quality.code().to_string()),
    };
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&settings).unwrap_or_default());
}
