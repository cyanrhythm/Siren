//! 元数据缓存：详情类接口（歌单 / 专辑 / 歌手）的 JSON 快照。
//!
//! 对齐 `docs/08-功能对照与清单.md` 的「元数据缓存」条目：
//! **在线请求成功即写、仅加载失败时读**——所以它不是"加速层"，而是"断网/接口抖动时
//! 页面还能打开"的兜底层，不会让用户看到过期数据（在线成功永远覆盖）。
//!
//! ## 键为什么不分账号
//!
//! 详情类实体的 id 本身就是账号唯一的（「我喜欢的音乐」「每日推荐」各账号 id 不同），
//! 所以 `playlist-<id>.json` 天然不会串号；公开歌单被两个账号看到时内容也一致。
//! 曾经按 `u<uid>` 分作用域，但**作用域切换晚于开机首屏加载**（会话恢复是异步的），
//! 于是首屏永远查不到自己刚存的快照——为了正确性去掉这一层。
//!
//! ## 容量
//!
//! 与封面/音频共用 [`DiskLru`]：64 MB 上限、48 MB 水位、按 mtime 淘汰。

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::disk_lru::{DiskLru, DiskStats};

/// 元数据缓存上限（JSON 都很小，64 MB 足够放上万个条目）。
pub(crate) const META_DISK_LIMIT: u64 = 64 * 1024 * 1024;
/// 淘汰水位。
pub(crate) const META_DISK_LOW_WATER: u64 = 48 * 1024 * 1024;

/// 水位必须低于上限，否则一次 trim 之后仍会反复触发。
const _: () = assert!(META_DISK_LOW_WATER < META_DISK_LIMIT);

#[derive(Clone)]
pub(crate) struct MetaCache {
    disk: Arc<DiskLru>,

    bytes: Arc<AtomicU64>,
    files: Arc<AtomicUsize>,
}

impl MetaCache {
    /// 打开系统缓存目录；取不到（无 HOME / 无权限）返回 `None`，页面退化为纯在线。
    pub(crate) fn open(handle: &tokio::runtime::Handle) -> Option<Self> {
        let dir = siren_platform::app_paths::metadata_cache_dir()?;
        Self::open_at(dir, handle)
    }

    /// 指定目录打开（单测用）。
    fn open_at(dir: std::path::PathBuf, handle: &tokio::runtime::Handle) -> Option<Self> {
        let disk = Arc::new(DiskLru::open(dir, META_DISK_LIMIT, META_DISK_LOW_WATER)?);
        log::info!("元数据缓存：{}", disk.dir().display());
        let bytes = Arc::new(AtomicU64::new(0));
        let files = Arc::new(AtomicUsize::new(0));
        disk.spawn_maintenance(handle, "元数据缓存", bytes.clone(), files.clone());
        Some(Self { disk, bytes, files })
    }

    /// 键 → 文件名（`<key>.json`，扁平目录 + 可读名字）。
    fn file_name(&self, key: &str) -> String {
        format!("{key}.json")
    }

    /// 读快照；文件不存在或内容损坏都返回 `None`（损坏时顺手删掉）。
    pub(crate) fn load<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let name = self.file_name(key);
        let bytes = self.disk.get(&name)?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                log::warn!("元数据缓存损坏，已删除 {name}：{err}");
                self.disk.remove(&name);
                None
            }
        }
    }

    /// 写快照（在线成功即调用）。序列化失败只记日志。
    pub(crate) fn save<T: Serialize>(&self, key: &str, value: &T) -> bool {
        let name = self.file_name(key);
        let bytes = match serde_json::to_vec(value) {
            Ok(bytes) => bytes,
            Err(err) => {
                log::warn!("元数据序列化失败（{name}）：{err}");
                return false;
            }
        };
        if !self.disk.put(&name, &bytes) {
            return false;
        }
        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        self.files.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// 用量快照（原子量，渲染路径可安全调用）。
    pub(crate) fn stats(&self) -> DiskStats {
        DiskStats {
            bytes: self.bytes.load(Ordering::Relaxed),
            files: self.files.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn limit(&self) -> u64 {
        self.disk.limit()
    }

    /// 清空缓存（设置页）；下次在线加载会重新写回。
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
    use serde::Deserialize;
    use std::time::SystemTime;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        id: i64,
        name: String,
        tracks: Vec<i64>,
    }

    fn sample(id: i64) -> Sample {
        Sample {
            id,
            name: format!("歌单 {id}"),
            tracks: vec![1, 2, 3],
        }
    }

    fn temp_cache(tag: &str) -> (MetaCache, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "siren-meta-cache-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("建 runtime");
        let cache = MetaCache::open_at(dir.clone(), runtime.handle()).expect("打开缓存");
        // runtime 需要存活（maintenance 任务挂在它上面）：泄漏一个守护线程的 runtime。
        std::mem::forget(runtime);
        (cache, dir)
    }

    #[test]
    fn save_then_load_roundtrips() {
        let (cache, dir) = temp_cache("roundtrip");
        let value = sample(7);
        assert!(cache.load::<Sample>("playlist-7").is_none());
        assert!(cache.save("playlist-7", &value));
        assert_eq!(cache.load::<Sample>("playlist-7"), Some(value));
        assert_eq!(cache.stats().files, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupted_entry_is_dropped() {
        let (cache, dir) = temp_cache("corrupt");
        let name = cache.file_name("album-5");
        std::fs::write(dir.join(&name), b"{ not json").unwrap();
        assert!(cache.load::<Sample>("album-5").is_none());
        assert!(!dir.join(&name).exists(), "损坏条目应被删除");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clear_removes_all_entries() {
        let (cache, dir) = temp_cache("clear");
        cache.save("playlist-1", &sample(1));
        cache.save("playlist-2", &sample(2));
        assert_eq!(cache.clear(), 2);
        assert_eq!(cache.stats().files, 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
