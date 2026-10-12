//! 播放抽象：与平台无关的播放控制逻辑。
//!
//! 对应 Melodia 的 `core/player/`（不含平台相关的 libmpv 绑定部分）。
//!
//! TODO: 定义
//! - `PlaybackController` trait（平台实现由 siren-platform 提供）
//! - 播放队列（顺序、随机、循环）
//! - 播放模式
//! - 播放状态流
//! - 播放来源解析（音质选择、换源）
