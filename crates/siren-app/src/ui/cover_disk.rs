//! 封面磁盘缓存（对齐 Melodia 的 `DiskCache` 500 MB 方案，见 `docs/缓存优化.md` P2.3）。
//!
//! ## 为什么要有这一层
//!
//! 内存上限（64 MB）解决的是常驻，但冷启动后每个封面仍要重新下载一遍。
//! 磁盘层把「已下载过的档位化封面」留下来，命中时直接读本地字节、不联网。
//!
//! ## 键与隐私
//!
//! 键是**档位化 URL**（含 `?param=WyH`，由 [`super::covers`] 的 `sized_url` 生成），
//! 文件名取其 128 位哈希——缓存目录里看不到原始 URL，只留下不可逆的哈希。
//! 因此这一层只缓存「公开封面/头像」这类可公开的内容，不含任何用户数据。
//!
//! ## 一致性
//!
//! 写入走「临时文件 + rename」，进程中途退出不会留下半个文件；
//! 读取时 touch 修改时间，淘汰按 mtime 从旧到新删到水位以下（与 Melodia 同法）。
//! 网易封面的 URL 就是内容地址（同一 URL 不会变内容），故不需要 TTL。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 磁盘缓存上限（对齐 Melodia `DISK_CACHE_MAX_BYTES`）。
pub(crate) const DISK_LIMIT_BYTES: u64 = 500 * 1024 * 1024;
/// 淘汰到此水位停手，避免每次写完都反复删几个文件（抖动）。
const DISK_LOW_WATER: u64 = 450 * 1024 * 1024;
/// 条目后缀：内容是原始压缩字节（JPEG/PNG），解码仍走 `image::load_from_memory`。
const ENTRY_EXT: &str = "img";
/// 临时文件后缀：rename 前的中间态。
const TMP_EXT: &str = "tmp";

/// 磁盘缓存用量快照。
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct DiskStats {
    pub(crate) bytes: u64,
    pub(crate) files: usize,
}

/// 封面磁盘缓存。目录取不到（无 HOME / 无权限）时由调用方按「无缓存」降级。
pub(crate) struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    /// 打开（必要时创建）图片缓存目录。失败返回 `None`，不影响内存缓存。
    pub(crate) fn open(dir: PathBuf) -> Option<Self> {
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self { dir })
    }

    /// 系统默认图片缓存目录；取不到则 `None`。
    pub(crate) fn open_default() -> Option<Self> {
        Self::open(siren_platform::app_paths::image_cache_dir()?)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// 键 → 文件路径（128 位哈希的十六进制）。
    fn path_for(&self, key: &str) -> PathBuf {
        self.dir
            .join(format!("{:016x}{:016x}.{ENTRY_EXT}", hash64(key, 1), hash64(key, 2)))
    }

    /// 读缓存；命中时 touch mtime（供 LRU 淘汰判断「最近用过」）。
    pub(crate) fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path_for(key);
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        touch(&path);
        Some(bytes)
    }

    /// 写缓存（临时文件 + rename，保证不出现半个文件）。失败静默忽略。
    pub(crate) fn put(&self, key: &str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let path = self.path_for(key);
        let tmp = path.with_extension(TMP_EXT);
        let written = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// 用量快照（遍历目录；调用方应放在 blocking 线程或周期任务里）。
    pub(crate) fn stats(&self) -> DiskStats {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return DiskStats::default();
        };
        let mut stats = DiskStats::default();
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            // 只统计正式条目；残留的临时文件由 `trim` 顺手清掉。
            if meta.is_file() && entry.path().extension().is_some_and(|ext| ext == ENTRY_EXT) {
                stats.bytes += meta.len();
                stats.files += 1;
            }
        }
        stats
    }

    /// 清空缓存目录（保留目录本身）。
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
            let is_tmp = path.extension().is_some_and(|ext| ext == TMP_EXT);
            if is_tmp {
                // 上次进程中途退出留下的中间态，直接删。
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            total = total.saturating_add(meta.len());
            files.push((path, mtime, meta.len()));
        }
        if total <= DISK_LIMIT_BYTES {
            return 0;
        }
        files.sort_by_key(|(_, mtime, _)| *mtime);
        let mut freed = 0;
        for (path, _, len) in files {
            if total <= DISK_LOW_WATER {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
                freed += len;
            }
        }
        freed
    }
}

/// 把文件修改时间设为现在（LRU 的「最近使用」）。
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// 稳定哈希：同一把 salt 下同一字符串总是同一个值（跨进程一致，`DefaultHasher` 不保证，
/// 故这里用 FNV-1a 手写，避免换 Rust 版本后整盘缓存失效）。
fn hash64(key: &str, salt: u8) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(salt);
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 目录用量 + 条目数（供日志与设置页）。
pub(crate) fn summary(stats: DiskStats) -> (f64, usize) {
    (stats.bytes as f64 / (1024.0 * 1024.0), stats.files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "siren-cover-disk-{tag}-{}-{}",
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
    fn same_key_same_path_and_distinct_keys_differ() {
        let dir = temp_dir("path");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        let a = cache.path_for("https://p1.music.126.net/x.jpg?param=64y64");
        let b = cache.path_for("https://p1.music.126.net/x.jpg?param=64y64");
        let c = cache.path_for("https://p1.music.126.net/x.jpg?param=128y128");
        assert_eq!(a, b, "同一键必须落到同一文件");
        assert_ne!(a, c, "不同档位必须是不同条目");
        assert!(a.starts_with(&dir));
        let name = a.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(".img"));
        assert_eq!(name.len(), 32 + 4, "128 位哈希 + 后缀：{name}");
        assert!(!name.contains("music.126.net"), "文件名不得泄露原始 URL");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn put_then_get_roundtrips_bytes() {
        let dir = temp_dir("roundtrip");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        let payload = b"\xFF\xD8fake-jpeg-bytes\xFF\xD9";
        assert!(cache.get("k1").is_none());
        cache.put("k1", payload);
        assert_eq!(cache.get("k1").as_deref(), Some(payload.as_slice()));
        let stats = cache.stats();
        assert_eq!(stats.files, 1);
        assert_eq!(stats.bytes, payload.len() as u64);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_keeps_total_under_limit_and_prefers_oldest() {
        let dir = temp_dir("trim");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        // 造 3 个「旧」条目（显式把 mtime 调旧）+ 1 个新条目，总量超过上限。
        let chunk = DISK_LIMIT_BYTES / 2 + 1;
        let big = vec![7u8; chunk as usize];
        for (i, name) in ["old-1", "old-2", "old-3"].iter().enumerate() {
            let path = dir.join(format!("{name}.img"));
            std::fs::write(&path, &big).unwrap();
            let old = SystemTime::now() - Duration::from_secs(60 * (10 - i as u64));
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(old).unwrap();
        }
        cache.put("fresh", b"new");
        let freed = cache.trim();
        assert!(freed > 0, "超上限必须淘汰");
        let stats = cache.stats();
        assert!(stats.bytes <= DISK_LOW_WATER, "淘汰后应回到水位以下：{}", stats.bytes);
        assert!(
            dir.join("fresh.img").exists() || cache.get("fresh").is_some(),
            "最新写入的条目应当保留"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_removes_entries_but_keeps_dir() {
        let dir = temp_dir("clear");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        cache.put("k1", b"one");
        cache.put("k2", b"two");
        assert_eq!(cache.stats().files, 2);
        assert_eq!(cache.clear(), 2);
        assert_eq!(cache.stats().files, 0);
        assert!(dir.exists(), "目录本身保留，下次写入不必重建");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_removes_leftover_tmp_files() {
        let dir = temp_dir("tmp");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        let tmp = dir.join("half-written.tmp");
        std::fs::write(&tmp, b"partial").unwrap();
        cache.trim();
        assert!(!tmp.exists(), "残留临时文件应被清掉");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stats_ignores_tmp_files() {
        let dir = temp_dir("stat");
        let cache = DiskCache::open(dir.clone()).expect("打开缓存");
        cache.put("k1", b"payload");
        std::fs::write(dir.join("x.tmp"), b"partial").unwrap();
        let stats = cache.stats();
        assert_eq!(stats.files, 1, "临时文件不计入用量");
        assert_eq!(summary(stats).1, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hash_is_stable_across_calls_and_salts_are_independent() {
        // 换 Rust 版本不能让整盘缓存失效，故断言具体值。
        assert_eq!(hash64("abc", 1), hash64("abc", 1));
        assert_ne!(hash64("abc", 1), hash64("abc", 2));
        assert_ne!(hash64("abc", 1), hash64("abd", 1));
        // FNV-1a 的已知值（salt=0 时 "a" → 0xaf63dc4c8601ec8c）
        assert_eq!(hash64("a", 0), 0xaf63_dc4c_8601_ec8c);
    }
}
