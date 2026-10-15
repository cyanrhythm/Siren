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
        Some(true)
    }
    fn eof_reached(&self) -> Option<bool> {
        Some(false)
    }
}
