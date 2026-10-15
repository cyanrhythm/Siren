//! 播放会话持久化，用于冷启动恢复。
//!
//! 拆成两个文件（与 [`crate::settings`] 同目录，`config_dir/`）：
//!
//! - `playback_queue.json` —— 队列 / 当前下标 / 模式；仅在**结构变化**时写（切歌、换队列、切模式）。
//! - `playback_state.json` —— 当前曲 id / 进度 / 时长；**高频**写（周期 5s、暂停、seek、退出）。
//!
//! 对齐 Melodia：`PlaybackStateStore` 队列与状态分开落盘，周期只写很小的状态。
//! 写入失败静默忽略。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Song;
use crate::player::{PlayMode, PlaySource};

fn config_path(name: &str) -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren").map(|dirs| dirs.config_dir().join(name))
}

fn write_json<T: Serialize>(name: &str, value: &T) {
    let Some(path) = config_path(name) else {
        return;
    };
    crate::persist::write_atomic(&path, &serde_json::to_string(value).unwrap_or_default());
}

fn read_json<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    let path = config_path(name)?;
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 队列会话：队列快照 + 当前下标 + 模式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSession {
    pub queue: Vec<Song>,
    pub index: usize,
    pub mode: PlayMode,
    /// 播放上下文标签（心动/漫游等）。
    #[serde(default)]
    pub play_context: Option<String>,
    /// 队列来源容器（歌单/专辑）。
    #[serde(default)]
    pub play_source: Option<PlaySource>,
}

impl QueueSession {
    const FILE: &'static str = "playback_queue.json";

    /// 读取上次队列（不存在或损坏返回 `None`）。
    pub fn load() -> Option<Self> {
        read_json(Self::FILE)
    }

    /// 删除已保存的队列会话（清空队列时调用，避免重启后旧队列“复活”）。
    pub fn clear() {
        if let Some(path) = config_path(Self::FILE) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// 写回磁盘。
    pub fn save(&self) {
        write_json(Self::FILE, self);
    }
}

/// 进度会话：当前曲 id + 进度 + 时长。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSession {
    pub song_id: i64,
    pub position_ms: i64,
    pub duration_ms: i64,
}

impl ProgressSession {
    const FILE: &'static str = "playback_state.json";

    /// 读取上次进度（不存在或损坏返回 `None`）。
    pub fn load() -> Option<Self> {
        read_json(Self::FILE)
    }

    /// 写回磁盘。
    pub fn save(&self) {
        write_json(Self::FILE, self);
    }
}
