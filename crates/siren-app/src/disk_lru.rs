//! 通用磁盘 LRU 缓存：目录 + 字节上限 + 按 mtime 淘汰。
//!
//! 封面上游（[`crate::ui::covers`]）与音频上游（[`crate::ui::audio_cache`]）共用这一套：
//! 两者都只需要「按名字读写一个文件、超上限删最旧的、能统计/清空」，差异只在
//! 目录、上限和文件命名——命名由调用方给（封面用哈希以免暴露 URL，音频用可读的 id-档位）。
//!
//! ## 约定
//!
//! - 写入走「临时文件 + rename」：进程中途退出不会留下半个文件；残留的 `.tmp` 由 `trim` 清掉。
//! - 读取 touch mtime，作为「最近使用」的依据（不额外维护索引文件）。
//! - 内容不可变（封面 URL / 音频地址一次写定），故不做 TTL，只靠容量淘汰。
//! - 所有方法都是阻塞 IO：调用方应放在 blocking 线程池或周期任务里，
//!   [`Self::spawn_maintenance`] 提供了现成的 60s 后台循环。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// 目录用量快照。
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct DiskStats {
    pub(crate) bytes: u64,
    pub(crate) files: usize,
}

/// 目录用量 → (MB, 文件数)。
pub(crate) fn summary(stats: DiskStats) -> (f64, usize) {
    (stats.bytes as f64 / (1024.0 * 1024.0), stats.files)
}

/// 条目后缀：临时文件（rename 前的中间态）。
const TMP_EXT: &str = "tmp";

pub(crate) struct DiskLru {
    dir: PathBuf,
    limit: u64,
    low_water: u64,
}

impl DiskLru {
    /// 打开（必要时创建）目录。失败返回 `None`，调用方按「无磁盘缓存」降级。
    pub(crate) fn open(dir: PathBuf, limit: u64, low_water: u64) -> Option<Self> {
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self {
            dir,
            limit,
            low_water: low_water.min(limit),
        })
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn limit(&self) -> u64 {
        self.limit
    }

    /// 条目的绝对路径（不保证存在）。
    pub(crate) fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// 读缓存；命中时 touch mtime（供 LRU 淘汰判断「最近用过」）。
    pub(crate) fn get(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.path_for(name);
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        touch(&path);
        Some(bytes)
    }

    /// 命中时返回路径并 touch（供需要把文件交给外部播放器的场景）。
    pub(crate) fn path_if_exists(&self, name: &str) -> Option<PathBuf> {
        let path = self.path_for(name);
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        touch(&path);
        Some(path)
    }

    /// 删除单个条目（内容损坏等场景）。返回是否真的删掉。
    pub(crate) fn remove(&self, name: &str) -> bool {
        std::fs::remove_file(self.path_for(name)).is_ok()
    }

    /// 写缓存（临时文件 + rename）。失败静默忽略，返回是否写入。
    pub(crate) fn put(&self, name: &str, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return false;
        }
        let path = self.path_for(name);
        let tmp = path.with_extension(TMP_EXT);
        let written = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        true
    }

    /// 用量快照（遍历目录；调用方应放在 blocking 线程池或周期任务里）。
    pub(crate) fn stats(&self) -> DiskStats {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return DiskStats::default();
        };
        let mut stats = DiskStats::default();
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            // 只统计正式条目；残留的临时文件由 `trim` 顺手清掉。
            if meta.is_file() && !entry.path().extension().is_some_and(|ext| ext == TMP_EXT) {
                stats.bytes += meta.len();
                stats.files += 1;
            }
        }
        stats
    }

    /// 清空目录（保留目录本身）。
    pub(crate) fn clear(&self) -> usize {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            if entry.path().is_file() && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// 超上限时按 mtime 从旧到新删到水位以下；同时清理残留临时文件。
    ///
    /// 返回删除的字节数（0 = 未触发）。
    pub(crate) fn trim(&self) -> u64 {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if path.extension().is_some_and(|ext| ext == TMP_EXT) {
                // 上次进程中途退出留下的中间态，直接删。
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            total = total.saturating_add(meta.len());
            files.push((path, mtime, meta.len()));
        }
        if total <= self.limit {
            return 0;
        }
        files.sort_by_key(|(_, mtime, _)| *mtime);
        let mut freed = 0;
        for (path, _, len) in files {
            if total <= self.low_water {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
                freed += len;
            }
        }
        freed
    }

    /// 后台维护：启动时立刻 `trim` 一次，之后每 60 s 刷新用量快照（供 `stats()` 只读）。
    ///
    /// `limit_label` 只用于日志；两个原子量分别写入字节与文件数，由调用方的 `stats()` 读取。
    pub(crate) fn spawn_maintenance(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
        label: &'static str,
        bytes: Arc<AtomicU64>,
        files: Arc<AtomicUsize>,
    ) {
        let cache = self.clone();
        handle.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(60));
            loop {
                let cache = cache.clone();
                let stats = tokio::task::spawn_blocking(move || {
                    // 先淘汰再统计，日志里的数字才是「稳定后的占用」。
                    cache.trim();
                    cache.stats()
                })
                .await
                .unwrap_or_default();
                bytes.store(stats.bytes, Ordering::Relaxed);
                files.store(stats.files, Ordering::Relaxed);
                log::debug!(
                    "{label}：{} 个文件 / {:.1} MB",
                    stats.files,
                    stats.bytes as f64 / (1024.0 * 1024.0)
                );
                tick.tick().await;
            }
        });
    }
}

/// 把文件修改时间设为现在（LRU 的「最近使用」）。
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "siren-disk-lru-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn put_then_get_roundtrips_bytes() {
        let dir = temp_dir("roundtrip");
        let cache = DiskLru::open(dir.clone(), 1024, 512).expect("打开缓存");
        assert!(cache.get("k1.image").is_none());
        assert!(cache.put("k1.image", b"\xFF\xD8fake\xFF\xD9"));
        assert_eq!(cache.get("k1.image").as_deref(), Some(&b"\xFF\xD8fake\xFF\xD9"[..]));
        assert_eq!(cache.stats().files, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn path_if_exists_touches_and_returns_absolute_path() {
        let dir = temp_dir("path");
        let cache = DiskLru::open(dir.clone(), 1024, 512).expect("打开缓存");
        assert!(cache.path_if_exists("a.audio").is_none());
        cache.put("a.audio", b"payload");
        let path = cache.path_if_exists("a.audio").expect("命中");
        assert!(path.starts_with(&dir));
        assert_eq!(std::fs::read(&path).unwrap(), b"payload");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_keeps_total_under_limit_and_prefers_oldest() {
        let dir = temp_dir("trim");
        let limit = 3 * 1024;
        let cache = DiskLru::open(dir.clone(), limit, limit / 2).expect("打开缓存");
        // 三个「旧」条目（各 1 KB，显式把 mtime 调旧）+ 一个新条目，总量超上限。
        let chunk = vec![7u8; 1024];
        for (i, name) in ["old-1.bin", "old-2.bin", "old-3.bin"].iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, &chunk).unwrap();
            let old = SystemTime::now() - Duration::from_secs(60 * (10 - i as u64));
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        cache.put("fresh.bin", b"new");
        let freed = cache.trim();
        assert!(freed > 0, "超上限必须淘汰");
        assert!(cache.stats().bytes <= limit / 2, "应回到水位以下");
        assert!(dir.join("fresh.bin").exists(), "最新写入的条目应当保留");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_removes_leftover_tmp_files() {
        let dir = temp_dir("tmp");
        let cache = DiskLru::open(dir.clone(), 1024, 512).expect("打开缓存");
        let tmp = dir.join("half-written.tmp");
        std::fs::write(&tmp, b"partial").unwrap();
        cache.trim();
        assert!(!tmp.exists(), "残留临时文件应被清掉");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stats_ignores_tmp_files() {
        let dir = temp_dir("stat");
        let cache = DiskLru::open(dir.clone(), 1024, 512).expect("打开缓存");
        cache.put("k1.bin", b"payload");
        std::fs::write(dir.join("x.tmp"), b"partial").unwrap();
        let stats = cache.stats();
        assert_eq!(stats.files, 1, "临时文件不计入用量");
        assert_eq!(summary(stats).1, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_removes_entries_but_keeps_dir() {
        let dir = temp_dir("clear");
        let cache = DiskLru::open(dir.clone(), 1024, 512).expect("打开缓存");
        cache.put("k1.bin", b"one");
        cache.put("k2.bin", b"two");
        assert_eq!(cache.stats().files, 2);
        assert_eq!(cache.clear(), 2);
        assert_eq!(cache.stats().files, 0);
        assert!(dir.exists(), "目录本身保留，下次写入不必重建");
        std::fs::remove_dir_all(&dir).ok();
    }
}
