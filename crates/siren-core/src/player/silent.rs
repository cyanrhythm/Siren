//! 空实现引擎：libmpv 不可用时的降级占位。
//!
//! 满足 Siren 的「libmpv 缺失时应用不崩溃」要求（见 `docs/04-迁移规格.md § 3.6`）：
//! 界面与队列仍可用，只是不出声。

use crate::player::PlaybackController;

/// 不出声的占位引擎。
#[derive(Debug, Default, Clone, Copy)]
pub struct SilentEngine;

impl PlaybackController for SilentEngine {
    fn load(&self, _url: &str) -> bool {
        // 降级占位：视为加载成功，界面/队列照常工作（只是不出声）。
        true
    }
    fn set_paused(&self, _paused: bool) {}
    fn stop(&self) {}
    fn seek_to_ms(&self, _position_ms: i64) {}
    fn set_volume(&self, _percent: f64) {}
    fn position_ms(&self) -> Option<i64> {
        None
    }
    fn duration_ms(&self) -> Option<i64> {
        None
    }
    fn is_paused(&self) -> Option<bool> {
        // 降级引擎无真实暂停态：返回 `None` 让上层沿用自身状态机。
        // 若返回 `Some(true)`，`tick` 每拍会把 `playing` 打回 false，表现为播放键失效（K6）。
        None
    }
    fn eof_reached(&self) -> Option<bool> {
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_paused_is_unknown_so_state_machine_keeps_playing() {
        // 降级引擎不报告暂停态：返回 `None`，否则 tick 每拍会把 playing 打回 false（K6）。
        assert_eq!(SilentEngine.is_paused(), None);
    }

    #[test]
    fn load_reports_accepted_so_ui_still_shows_playback() {
        // 缺 libmpv 时不应让「播放」变成错误：界面/队列照常走，只是不出声。
        assert!(SilentEngine.load("https://example.invalid/song.mp3"));
        assert!(SilentEngine.load_from("https://example.invalid/song.mp3", Some(12_000)));
    }

    #[test]
    fn eof_never_fires_so_auto_advance_does_not_spin() {
        // `eof_reached = false`：静音引擎没有真实播放进度，若报 true 会无限自动切歌。
        assert_eq!(SilentEngine.eof_reached(), Some(false));
    }

    #[test]
    fn position_and_duration_are_unknown() {
        // 返回 `None` 让上层保留自身状态（「无进度信息」≠「进度为 0」）。
        assert_eq!(SilentEngine.position_ms(), None);
        assert_eq!(SilentEngine.duration_ms(), None);
    }
}
