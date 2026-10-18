//! 音频缓存：播放过的歌曲字节落盘，再次播放直接读本地（也顺带支持断网播放）。
//!
//! 与封面层共用 [`crate::disk_lru::DiskLru`]，但**不做哈希命名**：音频是用户自己的内容，
//! 文件名用可读的 `{歌曲 id}-{音质档}.audio` 便于排查与手动清理。
//!
//! ## 键为什么带音质档
//!
//! 同一首歌在不同档位（`standard` / `exhigh` / `lossless` …）下是不同字节，
//! 混在一个文件里会串音质；档位取 `AudioQuality::code()`，与该次解析播放地址用的档位一致。
//!
//! ## 扩展名
//!
//! 固定 `.audio`：mpv 按内容探测容器，不依赖后缀；这样命中查询不必先知道真实格式。
//!
//! ## 与「下载」的区别
//!
//! 这只是**播放缓存**（可被容量淘汰、设置页可整体清除），不进音乐库、不产生用户可见的下载记录。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::disk_lru::{DiskLru, DiskStats};

/// 音频缓存上限：2 GB（约 400~500 首无损 / 2000 首 128k）。
pub(crate) const AUDIO_DISK_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
/// 淘汰水位：一次清到该值以下。
pub(crate) const AUDIO_DISK_LOW_WATER: u64 = 1800 * 1024 * 1024;

/// 水位必须低于上限，否则一次 trim 之后仍会反复触发。
const _: () = assert!(AUDIO_DISK_LOW_WATER < AUDIO_DISK_LIMIT);

/// 播放缓存句柄（挂在 `Playback` 上）。
#[derive(Clone)]
pub(crate) struct AudioCache {
    disk: Arc<DiskLru>,
    bytes: Arc<AtomicU64>,
    files: Arc<AtomicUsize>,
    /// 正在下载的键：同一首重复触发只下载一次。
    inflight: Arc<Mutex<HashSet<String>>>,
}

impl AudioCache {
    /// 打开系统音频缓存目录；取不到（无 HOME / 无权限）返回 `None`，播放功能不受影响。
    pub(crate) fn open(handle: &tokio::runtime::Handle) -> Option<Self> {
        let dir = siren_platform::app_paths::audio_cache_dir()?;
        let disk = Arc::new(DiskLru::open(dir, AUDIO_DISK_LIMIT, AUDIO_DISK_LOW_WATER)?);
        log::info!("音频缓存：{}", disk.dir().display());
        let bytes = Arc::new(AtomicU64::new(0));
        let files = Arc::new(AtomicUsize::new(0));
        disk.spawn_maintenance(handle, "音频缓存", bytes.clone(), files.clone());
        Some(Self {
            disk,
            bytes,
            files,
            inflight: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// 条目文件名：`{歌曲 id}-{音质档}.audio`。
    pub(crate) fn file_name(song_id: i64, level: &str) -> String {
        format!("{song_id}-{level}.audio")
    }

    /// 命中则返回本地路径（并 touch，供 LRU 判断最近使用）。
    pub(crate) fn lookup(&self, song_id: i64, level: &str) -> Option<PathBuf> {
        self.disk.path_if_exists(&Self::file_name(song_id, level))
    }

    /// 把字节写入缓存。同一 `(id, 档位)` 并发调用只写一次。
    ///
    /// 返回 `true` 表示本次真的写入了（供日志/统计）。
    pub(crate) fn store(&self, song_id: i64, level: &str, bytes: &[u8]) -> bool {
        let name = Self::file_name(song_id, level);
        if !self.inflight.lock().unwrap().insert(name.clone()) {
            return false;
        }
        let written = self.disk.put(&name, bytes);
        self.inflight.lock().unwrap().remove(&name);
        if written {
            self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            self.files.fetch_add(1, Ordering::Relaxed);
        }
        written
    }

    /// 用量快照（原子量，渲染路径可安全调用）。
    pub(crate) fn stats(&self) -> DiskStats {
        DiskStats {
            bytes: self.bytes.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
        }
    }

    /// 上限（供设置页展示）。
    pub(crate) fn limit(&self) -> u64 {
        self.disk.limit()
    }

    /// 清空缓存（设置页）；下次播放会重新下载。
    pub(crate) fn clear(&self) -> usize {
        let removed = self.disk.clear();
        self.bytes.store(0, Ordering::Relaxed);
        self.files.store(0, Ordering::Relaxed);
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_includes_song_and_quality() {
        assert_eq!(AudioCache::file_name(123, "exhigh"), "123-exhigh.audio");
        // 同一首歌不同档位必须是不同条目（否则会串音质）。
        assert_ne!(
            AudioCache::file_name(123, "exhigh"),
            AudioCache::file_name(123, "lossless")
        );
        // 不同歌曲互不覆盖。
        assert_ne!(
            AudioCache::file_name(123, "exhigh"),
            AudioCache::file_name(124, "exhigh")
        );
    }

    #[test]
    fn store_and_lookup_roundtrip_through_disk_lru() {
        let dir = std::env::temp_dir().join(format!("siren-audio-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let disk = Arc::new(DiskLru::open(dir.clone(), 1024 * 1024, 512 * 1024).unwrap());
        let cache = AudioCache {
            disk,
            bytes: Arc::new(AtomicU64::new(0)),
            files: Arc::new(AtomicUsize::new(0)),
            inflight: Arc::new(Mutex::new(HashSet::new())),
        };
        assert!(cache.lookup(7, "exhigh").is_none());
        assert!(cache.store(7, "exhigh", b"audio-bytes"));
        let path = cache.lookup(7, "exhigh").expect("应当命中");
        assert!(path.starts_with(&dir));
        assert_eq!(std::fs::read(&path).unwrap(), b"audio-bytes");
        assert_eq!(cache.stats().files, 1);
        assert_eq!(cache.clear(), 1);
        assert!(cache.lookup(7, "exhigh").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_is_idempotent_under_concurrency_guard() {
        let dir = std::env::temp_dir().join(format!("siren-audio-cache-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let disk = Arc::new(DiskLru::open(dir.clone(), 1024 * 1024, 512 * 1024).unwrap());
        let cache = AudioCache {
            disk,
            bytes: Arc::new(AtomicU64::new(0)),
            files: Arc::new(AtomicUsize::new(0)),
            inflight: Arc::new(Mutex::new(HashSet::new())),
        };
        // 第一次写入成功；模拟「已有一个在途」时第二次被去重。
        assert!(cache.store(9, "standard", b"abc"));
        cache
            .inflight
            .lock()
            .unwrap()
            .insert(AudioCache::file_name(9, "standard"));
        assert!(!cache.store(9, "standard", b"abc"), "在途时不应重复写");
        std::fs::remove_dir_all(&dir).ok();
    }
}
