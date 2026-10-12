//! 播放抽象：与平台无关的播放控制逻辑。
//!
//! 对应 Melodia 的 `core/player/`。
//!
//! ## 模块
//!
//! - [`mpv`]：libmpv 运行时绑定与纯音频引擎（跨平台，无 `cfg`）。
//!
//! TODO: 定义
//! - `PlaybackController` trait
//! - 播放队列（顺序、随机、循环）
//! - 播放模式
//! - 播放状态流
//! - 播放来源解析（音质选择、换源）

pub mod mpv;
