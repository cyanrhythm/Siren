//! # siren-core
//!
//! 跨平台核心层：与平台无关的业务逻辑。
//!
//! ## 职责
//!
//! - 网络请求与签名（网易云 API）
//! - 加密算法（WeApi / EApi / LinuxApi）
//! - 数据模型（歌曲、歌单、专辑、歌手、播客等）
//! - 播放抽象（播放队列、状态、模式）
//! - 歌词解析（LRC / QRC / TTML）
//!
//! ## 约束
//!
//! **本 crate 不得出现 `#[cfg(target_os)]`** —— 平台差异必须收敛在 `siren-platform`。
//!
//! 详见 `docs/01-架构设计.md`。

pub mod auth;
pub mod crypto;
pub mod error;
pub mod logging;
pub mod lyrics;
pub mod model;
pub mod network;
pub mod persist;
pub mod playback_session;
pub mod player;
pub mod recent_playlists;
pub mod settings;

pub use error::{Error, Result};
