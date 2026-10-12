//! 应用目录能力。
//!
//! ## 实现方式
//!
//! **优先使用 `directories` crate**，而非自建实现。
//!
//! Siren 是全新项目，无 Melodia 的历史包袱——
//! Melodia 曾因 `directories-jvm` 会在 Windows 多插一层目录而否决它，
//! 但那是因为其已有用户数据在旧路径。详见 `docs/04-迁移规格.md § 2.1`。
//!
//! ## 目录约定
//!
//! | 用途 | 位置 |
//! |---|---|
//! | 数据（配置、登录态） | `data_dir` |
//! | 日志 | `data_dir/logs/` |
//! | 图片缓存 | `cache_dir/image_cache/` |
//! | 音频缓存 | `cache_dir/audio_cache/` |
//! | 元数据缓存 | `cache_dir/meta_cache/` |
//! | 下载临时 | `cache_dir/download_tmp/` |
//! | 默认下载目录 | `~/Music/Siren/` |
//!
//! ## 三平台输出示例
//!
//! ```text
//! 数据目录：
//!   Windows  %APPDATA%\Siren
//!   Linux    ~/.config/siren
//!   macOS    ~/Library/Application Support/com.cyanrhythm.Siren
//!
//! 缓存目录：
//!   Windows  %LOCALAPPDATA%\Siren\cache
//!   Linux    ~/.cache/siren
//!   macOS    ~/Library/Caches/com.cyanrhythm.Siren
//! ```

// TODO: 定义 AppPaths trait
// - data_dir() / cache_dir()
// - log_dir() / image_cache_dir() / audio_cache_dir() / metadata_cache_dir()
// - download_temp_dir() / default_download_dir()
// - preferences_file(name)
