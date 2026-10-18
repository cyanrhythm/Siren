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

use std::path::PathBuf;

/// 项目目录（`directories` 统一三平台差异，本 crate 是唯一允许出现平台分支的地方）。
///
/// 取不到（无 HOME / 无权限）时返回 `None`，调用方按「无缓存」降级。
fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "cyanrhythm", "Siren")
}

/// 数据目录（配置、登录态等需要持久化且用户可见的内容）。
pub fn data_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.data_dir().to_path_buf())
}

/// 缓存根目录（可随时删除、不参与备份）。
pub fn cache_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.cache_dir().to_path_buf())
}

/// 图片缓存目录（封面 / 头像的磁盘缓存，见 `docs/缓存优化.md` P2.3）。
pub fn image_cache_dir() -> Option<PathBuf> {
    cache_dir().map(|dir| dir.join("image_cache"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓存目录必须位于 `cache_dir` 下、且与数据目录分开（缓存可清、数据不可）。
    #[test]
    fn image_cache_lives_under_cache_dir() {
        let (Some(cache), Some(images)) = (cache_dir(), image_cache_dir()) else {
            return; // 无 HOME 的环境（CI 容器）跳过
        };
        assert_eq!(images, cache.join("image_cache"));
        assert!(images.starts_with(&cache));
        if let Some(data) = data_dir() {
            assert!(!images.starts_with(&data), "缓存不应写进数据目录");
        }
    }
}
