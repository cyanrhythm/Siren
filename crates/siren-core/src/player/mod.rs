//! 播放抽象：与平台无关的播放控制逻辑。
//!
//! 对应 Melodia 的 `core/player/`。
//!
//! ## 模块
//!
//! - [`mpv`]：libmpv 运行时绑定与纯音频引擎（跨平台，无 `cfg`）。
//! - [`queue`]：播放队列与播放模式（纯逻辑）。
//!
//! ## 待补充
//!
//! - 播放状态流 / 订阅机制
//! - 播放来源解析（音质选择、换源）
//! - 交叉淡入、流式缓存

pub mod mpv;
pub mod playback;
pub mod queue;
pub mod silent;

pub use mpv::{AudioDevice, Mpv};
pub use playback::{PlaybackState, Player};
pub use queue::{PlayMode, PlaySource, PlaySourceKind, PlayerQueue};
pub use silent::SilentEngine;

/// 播放控制抽象：由具体音频引擎实现（当前为 libmpv）。
///
/// 方法均取 `&self`（引擎内部串行化），便于跨线程共享。
pub trait PlaybackController: Send + Sync {
    /// 加载并播放地址；返回是否成功交给引擎（失败时上层不应显示为「播放中」）。
    fn load(&self, url: &str) -> bool;
    /// 加载地址并从指定位置开始播放（`None` = 从头）；返回是否成功。
    ///
    /// 引擎不支持时退化为 [`load`](Self::load)。实现应让引擎在**加载完成后**再跳转，
    /// 避免加载前 seek 被丢弃（冷启动恢复进度即依赖此语义）。
    fn load_from(&self, url: &str, start_ms: Option<i64>) -> bool {
        let _ = start_ms;
        self.load(url)
    }
    /// 设置暂停状态。
    fn set_paused(&self, paused: bool);
    /// 停止播放。
    fn stop(&self);
    /// 跳到绝对位置（毫秒）。
    fn seek_to_ms(&self, position_ms: i64);
    /// 设置音量百分比（0..=100）。
    fn set_volume(&self, percent: f64);
    /// 当前位置（毫秒）。
    fn position_ms(&self) -> Option<i64>;
    /// 总时长（毫秒）。
    fn duration_ms(&self) -> Option<i64>;
    /// 是否暂停；引擎不支持时返回 `None`。
    fn is_paused(&self) -> Option<bool>;
    /// 是否已到文件结尾；引擎不支持时返回 `None`。
    fn eof_reached(&self) -> Option<bool>;
}

/// 按平台前缀过滤音频设备（前缀由 `siren-platform` 提供）。
pub fn filter_audio_devices(devices: Vec<AudioDevice>, prefixes: &[&str]) -> Vec<AudioDevice> {
    devices
        .into_iter()
        .filter(|d| prefixes.iter().any(|p| d.name.starts_with(p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str) -> AudioDevice {
        AudioDevice {
            name: name.to_string(),
            description: name.to_string(),
        }
    }

    #[test]
    fn filter_keeps_only_matching_prefixes_in_order() {
        let devices = vec![
            dev("wasapi/{aaa}"),
            dev("auto"),
            dev("wasapi/{bbb}"),
            dev("pipewire/out"),
        ];
        let kept = filter_audio_devices(devices, &["wasapi/"]);
        assert_eq!(
            kept.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["wasapi/{aaa}", "wasapi/{bbb}"]
        );
    }

    #[test]
    fn filter_accepts_multiple_prefixes_and_empty_input() {
        let devices = vec![dev("coreaudio/x"), dev("pipewire/y"), dev("alsa/z")];
        let kept = filter_audio_devices(devices, &["coreaudio/", "alsa/"]);
        assert_eq!(kept.len(), 2);
        // 空前缀列表 = 不过滤任何设备，但也绝不能 panic 或返回垃圾。
        assert!(filter_audio_devices(vec![dev("auto")], &[]).is_empty());
        assert!(filter_audio_devices(vec![], &["wasapi/"]).is_empty());
    }
}
