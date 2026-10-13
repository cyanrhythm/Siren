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
    /// 加载并播放地址。
    fn load(&self, url: &str);
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
