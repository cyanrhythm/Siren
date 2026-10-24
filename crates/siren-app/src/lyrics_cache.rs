//! 歌词磁盘缓存：`cache_dir/lyrics_cache/`，存**解析后的歌词行**。
//!
//! 对齐 `docs/08` 的「歌词缓存」条目：TTL 7 天、空结果负缓存 12h、上限 75 MB、LRU 修剪。
//!
//! ## 为什么存结构化结果而不是 TTML 原文
//!
//! 解析（含 yrc 逐字、tlyric 译文合并）在 `api::lyrics` 里已经完成，缓存解析结果
//! 与缓存原文对我们的渲染器等价；代价是解析器升级后旧条目仍按旧规则解释——
//! 因此每次解析规则变化可以把上限之外的条目自然淘汰，或让 7 天 TTL 兜底。
//!
//! ## 时效
//!
//! 每条带 `fetched_at`（毫秒时间戳）。命中还要看 TTL：
//! - 正常歌词：7 天；
//! - 空结果（接口返回空 / 无版权）：12 小时，避免每次播放都重试一次必然失败的请求。
//!
//! 文件 mtime 只用于 LRU 淘汰（读取会 touch），与 TTL 无关。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use siren_core::lyrics::LyricLine;

use crate::disk_lru::{DiskLru, DiskStats};

/// 上限 75 MB（对齐 Melodia `LyricsCache`）。
pub(crate) const LYRICS_DISK_LIMIT: u64 = 75 * 1024 * 1024;
/// 淘汰水位。
pub(crate) const LYRICS_DISK_LOW_WATER: u64 = 60 * 1024 * 1024;
/// 正常歌词的 TTL。
pub(crate) const LYRICS_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// 空结果（无歌词）的负缓存 TTL。
pub(crate) const LYRICS_NEGATIVE_TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// 水位必须低于上限。
const _: () = assert!(LYRICS_DISK_LOW_WATER < LYRICS_DISK_LIMIT);

/// 磁盘条目：抓取时间 + 歌词行（空 = 确认「没有歌词」）。
#[derive(Serialize, Deserialize)]
struct Entry {
    /// 抓取时刻（Unix 毫秒）。
    fetched_at: u64,
    lines: Vec<LyricLine>,
}

/// 一次命中的结果（与「没有缓存」区分开）。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Cached {
    /// 有歌词。
    Lines(Vec<LyricLine>),
    /// 12 小时内的负缓存：确认没有歌词。
    Empty,
}

#[derive(Clone)]
pub(crate) struct LyricsCache {
    disk: Arc<DiskLru>,
    bytes: Arc<AtomicU64>,
    files: Arc<AtomicUsize>,
}

impl LyricsCache {
    /// 打开系统歌词缓存目录；取不到返回 `None`，播放退化为每次请求。
    pub(crate) fn open(handle: &tokio::runtime::Handle) -> Option<Self> {
        let dir = siren_platform::app_paths::lyrics_cache_dir()?;
        Self::open_at(dir, handle)
    }

    fn open_at(dir: std::path::PathBuf, handle: &tokio::runtime::Handle) -> Option<Self> {
        let disk = Arc::new(DiskLru::open(dir, LYRICS_DISK_LIMIT, LYRICS_DISK_LOW_WATER)?);
        log::info!("歌词缓存：{}", disk.dir().display());
        let bytes = Arc::new(AtomicU64::new(0));
        let files = Arc::new(AtomicUsize::new(0));
        disk.spawn_maintenance(handle, "歌词缓存", bytes.clone(), files.clone());
        Some(Self { disk, bytes, files })
    }

    /// 键 → 文件名（歌曲 id）。
    fn file_name(song_id: i64) -> String {
        format!("{song_id}.json")
    }

    /// 读缓存；过期、损坏或不存在都返回 `None`（调用方据此重新请求）。
    pub(crate) fn load(&self, song_id: i64) -> Option<Cached> {
        let name = Self::file_name(song_id);
        let bytes = self.disk.get(&name)?;
        let entry: Entry = match serde_json::from_slice(&bytes) {
            Ok(entry) => entry,
            Err(err) => {
                log::warn!("歌词缓存损坏，已删除 {name}：{err}");
                self.disk.remove(&name);
                return None;
            }
        };
        let age = now_ms().saturating_sub(entry.fetched_at);
        let ttl = if entry.lines.is_empty() {
            // 空结果只配短 TTL：无版权/未上架的曲目几天后可能就有了。
            LYRICS_NEGATIVE_TTL
        } else {
            LYRICS_TTL
        };
        if age > ttl.as_millis() as u64 {
            log::trace!("歌词缓存过期：{name}（{} 秒前）", age / 1000);
            return None;
        }
        Some(if entry.lines.is_empty() {
            Cached::Empty
        } else {
            Cached::Lines(entry.lines)
        })
    }

    /// 写缓存（请求完成后调用；空结果同样写入，供负缓存使用）。
    pub(crate) fn save(&self, song_id: i64, lines: &[LyricLine]) -> bool {
        let entry = Entry {
            fetched_at: now_ms(),
            lines: lines.to_vec(),
        };
        let bytes = match serde_json::to_vec(&entry) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("歌词序列化失败 id={song_id}：{err}");
                return false;
            }
        };
        let name = Self::file_name(song_id);
        if !self.disk.put(&name, &bytes) {
            return false;
        }
        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        self.files.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// 用量快照（原子量）。
    pub(crate) fn stats(&self) -> DiskStats {
        DiskStats {
            bytes: self.bytes.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn limit(&self) -> u64 {
        self.disk.limit()
    }

    /// 清空缓存（设置页）。
    pub(crate) fn clear(&self) -> usize {
        let removed = self.disk.clear();
        self.bytes.store(0, Ordering::Relaxed);
        self.files.store(0, Ordering::Relaxed);
        removed
    }
}

/// 当前 Unix 毫秒。
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, time_ms: i64) -> LyricLine {
        LyricLine {
            time_ms,
            duration_ms: 1000,
            text: text.to_string(),
            translation: None,
            words: Vec::new(),
        }
    }

    fn temp_cache(tag: &str) -> (LyricsCache, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "siren-lyrics-cache-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("建 runtime");
        let cache = LyricsCache::open_at(dir.clone(), runtime.handle()).expect("打开缓存");
        std::mem::forget(runtime);
        (cache, dir)
    }

    /// 直接写一条指定抓取时间的条目（模拟过期 / 负缓存）。
    fn write_entry(cache: &LyricsCache, song_id: i64, fetched_at: u64, lines: &[LyricLine]) {
        let entry = Entry {
            fetched_at,
            lines: lines.to_vec(),
        };
        let bytes = serde_json::to_vec(&entry).unwrap();
        cache.disk.put(&LyricsCache::file_name(song_id), &bytes);
    }

    #[test]
    fn save_then_load_roundtrips() {
        let (cache, dir) = temp_cache("roundtrip");
        assert_eq!(cache.load(42), None);
        assert!(cache.save(42, &[line("第一行", 1000), line("第二行", 2000)]));
        assert_eq!(
            cache.load(42),
            Some(Cached::Lines(vec![line("第一行", 1000), line("第二行", 2000)]))
        );
        assert_eq!(cache.stats().files, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_result_is_negative_cached() {
        let (cache, dir) = temp_cache("negative");
        assert!(cache.save(7, &[]));
        assert_eq!(cache.load(7), Some(Cached::Empty));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn positive_entry_expires_after_seven_days() {
        let (cache, dir) = temp_cache("ttl");
        let lines = [line("x", 0)];
        // 6 天前：仍有效。
        write_entry(&cache, 1, now_ms() - 6 * 24 * 3600 * 1000, &lines);
        assert!(matches!(cache.load(1), Some(Cached::Lines(_))));
        // 8 天前：过期。
        write_entry(&cache, 2, now_ms() - 8 * 24 * 3600 * 1000, &lines);
        assert_eq!(cache.load(2), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn negative_entry_expires_after_twelve_hours() {
        let (cache, dir) = temp_cache("negttl");
        // 11 小时前：仍在负缓存窗口。
        write_entry(&cache, 3, now_ms() - 11 * 3600 * 1000, &[]);
        assert_eq!(cache.load(3), Some(Cached::Empty));
        // 13 小时前：过期，应重新请求。
        write_entry(&cache, 4, now_ms() - 13 * 3600 * 1000, &[]);
        assert_eq!(cache.load(4), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_entry_is_dropped() {
        let (cache, dir) = temp_cache("corrupt");
        let name = LyricsCache::file_name(5);
        std::fs::write(dir.join(&name), b"not json").unwrap();
        assert_eq!(cache.load(5), None);
        assert!(!dir.join(&name).exists(), "损坏条目应被删除");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_removes_entries() {
        let (cache, dir) = temp_cache("clear");
        cache.save(1, &[line("a", 0)]);
        cache.save(2, &[line("b", 0)]);
        assert_eq!(cache.clear(), 2);
        assert_eq!(cache.stats().files, 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
