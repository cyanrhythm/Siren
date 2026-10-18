//! 用户设置持久化（`config_dir/settings.json`）。
//!
//! 与 [`crate::auth`] 的会话文件同目录。写入失败静默忽略，不阻断业务流程。
//!
//! 应用层以全局 [`SettingsStore`]（见 `siren-app`）持有本模型的唯一实例，
//! 各处以 store 读写，避免同一文件被多处各自拥有。
//! 窗口位置/大小在 UI 图建立前读取，故保留引导期自由函数
//! [`load_window_bounds`] / [`save_window_bounds`]。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::AudioQuality;

/// 设置文件路径：`config_dir/settings.json`。
fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren").map(|dirs| dirs.config_dir().join("settings.json"))
}

/// 全部用户设置。字段均为 `Option`，缺省走各自的默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// 音质档位代码（如 `exhigh`）。
    quality: Option<String>,
    /// 是否启用系统媒体控制（SMTC / MPRIS），默认开启。
    media_keys: Option<bool>,
    /// 关闭主窗口时最小化到系统托盘（而非退出），默认开启。
    close_to_tray: Option<bool>,
    /// 播放过的歌曲是否缓存到本地磁盘（`cache_dir/audio_cache`），默认开启。
    audio_cache: Option<bool>,
    /// 正在播放卡片顺序（代码列表，见 [`DEFAULT_CARDS`]）。
    cards: Option<Vec<String>>,
    /// 正在播放面板中隐藏的卡片（代码）。
    hidden_cards: Option<Vec<String>>,
    /// 右侧栏宽度（像素）。
    np_width: Option<f32>,
    /// 歌曲详情页（正在播放面板）是否显示滚动条，默认显示。
    np_scrollbar: Option<bool>,
    /// 音量百分比（0–100）。
    volume: Option<f64>,
    /// 上次窗口位置与大小（`x,y,w,h`）。
    window: Option<[f32; 4]>,
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

impl Settings {
    /// 从磁盘读取（不存在或损坏时返回默认值）。
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// 写回磁盘（原子替换；失败静默）。
    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        crate::persist::write_atomic(&path, &serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// 音质档位（缺省为默认档）。
    pub fn quality(&self) -> AudioQuality {
        self.quality.as_deref().map(AudioQuality::from_code).unwrap_or_default()
    }

    /// 设置音质档位并落盘。
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = Some(quality.code().to_string());
        self.save();
    }

    /// 是否启用系统媒体控制（默认开启）。
    pub fn media_keys(&self) -> bool {
        self.media_keys.unwrap_or(true)
    }

    /// 设置系统媒体控制开关并落盘。
    pub fn set_media_keys(&mut self, enabled: bool) {
        self.media_keys = Some(enabled);
        self.save();
    }

    /// 关闭主窗口时是否最小化到托盘（默认开启）。
    pub fn close_to_tray(&self) -> bool {
        self.close_to_tray.unwrap_or(true)
    }

    /// 设置关闭到托盘并落盘。
    pub fn set_close_to_tray(&mut self, enabled: bool) {
        self.close_to_tray = Some(enabled);
        self.save();
    }

    /// 是否把播放过的歌曲缓存到本地（默认开启）。关闭后只流式播放、不落盘。
    pub fn audio_cache(&self) -> bool {
        self.audio_cache.unwrap_or(true)
    }

    /// 设置音频缓存开关并落盘。
    pub fn set_audio_cache(&mut self, enabled: bool) {
        self.audio_cache = Some(enabled);
        self.save();
    }

    /// 正在播放卡片顺序（缺省为 [`DEFAULT_CARDS`]）。
    pub fn cards(&self) -> Vec<String> {
        match &self.cards {
            Some(cards) if !cards.is_empty() => cards.clone(),
            _ => DEFAULT_CARDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// 保存正在播放卡片顺序并落盘。
    pub fn set_cards(&mut self, cards: Vec<String>) {
        self.cards = Some(cards);
        self.save();
    }

    /// 正在播放卡片的完整顺序（含隐藏项；补齐 [`DEFAULT_CARDS`] 中缺失的代码）。
    pub fn card_order(&self) -> Vec<String> {
        let mut order = self.cards();
        for code in DEFAULT_CARDS {
            if !order.iter().any(|c| c == code) {
                order.push(code.to_string());
            }
        }
        let mut seen = std::collections::HashSet::new();
        order.retain(|c| seen.insert(c.clone()));
        order
    }

    /// 隐藏的卡片代码列表。
    pub fn hidden_cards(&self) -> Vec<String> {
        self.hidden_cards.clone().unwrap_or_default()
    }

    /// 某卡片是否隐藏。
    pub fn is_card_hidden(&self, code: &str) -> bool {
        self.hidden_cards().iter().any(|c| c == code)
    }

    /// 保存完整卡片顺序并落盘。
    pub fn set_card_order(&mut self, order: Vec<String>) {
        self.cards = Some(order);
        self.save();
    }

    /// 保存隐藏卡片集合并落盘。
    pub fn set_hidden_cards(&mut self, hidden: Vec<String>) {
        self.hidden_cards = Some(hidden);
        self.save();
    }

    /// 右侧栏宽度（夹在 [`NP_WIDTH_MIN`]..[`NP_WIDTH_MAX`]）。
    pub fn np_width(&self) -> f32 {
        self.np_width
            .unwrap_or(NP_WIDTH_DEFAULT)
            .clamp(NP_WIDTH_MIN, NP_WIDTH_MAX)
    }

    /// 保存右侧栏宽度并落盘。
    pub fn set_np_width(&mut self, width: f32) {
        self.np_width = Some(width.clamp(NP_WIDTH_MIN, NP_WIDTH_MAX));
        self.save();
    }

    /// 仅更新内存中的宽度（不落盘）；拖动过程中用，结束时再 [`save`](Self::save)。
    pub fn set_np_width_live(&mut self, width: f32) {
        self.np_width = Some(width.clamp(NP_WIDTH_MIN, NP_WIDTH_MAX));
    }

    /// 歌曲详情页（正在播放面板）是否显示滚动条（默认显示）。
    pub fn np_scrollbar(&self) -> bool {
        self.np_scrollbar.unwrap_or(true)
    }

    /// 设置歌曲详情页滚动条显隐并落盘。
    pub fn set_np_scrollbar(&mut self, enabled: bool) {
        self.np_scrollbar = Some(enabled);
        self.save();
    }

    /// 音量百分比（0–100，默认 80）。
    pub fn volume(&self) -> f64 {
        self.volume.unwrap_or(80.0).clamp(0.0, 100.0)
    }

    /// 设置音量并落盘。
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = Some(volume.clamp(0.0, 100.0));
        self.save();
    }

    /// 上次窗口位置与大小 `(x, y, w, h)`（尺寸过小时视为无效）。
    pub fn window_bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let [x, y, w, h] = self.window?;
        (w >= 320.0 && h >= 240.0).then_some((x, y, w, h))
    }

    /// 保存窗口位置与大小。
    pub fn set_window_bounds(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.window = Some([x, y, w, h]);
        self.save();
    }
}

/// 读取上次窗口位置与大小（引导期使用，见本模块文档）。
pub fn load_window_bounds() -> Option<(f32, f32, f32, f32)> {
    Settings::load().window_bounds()
}

/// 保存窗口位置与大小（引导期使用）。
pub fn save_window_bounds(x: f32, y: f32, w: f32, h: f32) {
    let mut settings = Settings::load();
    settings.set_window_bounds(x, y, w, h);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert_eq!(settings.quality(), AudioQuality::default());
        assert!(settings.media_keys());
        assert_eq!(settings.np_width(), NP_WIDTH_DEFAULT);
        assert!(settings.np_scrollbar());
        assert_eq!(settings.cards().len(), DEFAULT_CARDS.len());
        assert!(settings.window_bounds().is_none());
    }

    #[test]
    fn np_scrollbar_toggles() {
        let mut settings = Settings::default();
        assert!(settings.np_scrollbar());
        settings.np_scrollbar = Some(false);
        assert!(!settings.np_scrollbar());
    }

    #[test]
    fn np_width_is_clamped() {
        let low = Settings {
            np_width: Some(10.0),
            ..Default::default()
        };
        assert_eq!(low.np_width(), NP_WIDTH_MIN);
        let high = Settings {
            np_width: Some(9999.0),
            ..Default::default()
        };
        assert_eq!(high.np_width(), NP_WIDTH_MAX);
    }
}
