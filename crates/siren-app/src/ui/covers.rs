//! 封面加载器：按尺寸分档 + 字节上限 LRU + 档位回退。
//!
//! 抓取由 [`CoverLoader::prefetch`] 在**数据加载完成时**触发；
//! [`CoverLoader::image`] 仅做只读查询（命中缓存渲染图片，否则占位），
//! 不在 render 路径中发起网络请求（对齐 coding-guides 的副作用规则）。
//!
//! ## 键与尺寸
//!
//! 档位由「逻辑尺寸 × DPR」向上取到 64/128/256/512/1024：
//! 2× 屏上 40px 的行会请求 128 档，避免高 DPI 下糊边。DPR 由
//! `Root::render` 每帧写入，预取与渲染因此共用同一套键。
//!
//! ## 两层常驻与淘汰
//!
//! 一张封面在内存里有两份：本模块的压缩字节（`Arc<Image>`）与 GPUI 资产表里
//! 的解码位图（`App::loading_assets`，只增不删）。只丢前者等于没回收，所以
//! 淘汰必须同时调 `ImageSource::remove_asset` —— 且只能在 **UI 线程**做。
//! 因此 [`CoverLoader::evict`] 由预取路径与 60s 定时器驱动，tokio 任务只负责写入。
//!
//! 淘汰按 `last_seen`（每次渲染命中都会刷新）选「至少 10s 没被碰过」的条目，
//! 屏上正在用的图不会刚被删掉又被下一帧重新解码（那会变成逐帧抖动）。
//!
//! ## 磁盘层
//!
//! 内存未命中时先查 [`super::cover_disk::DiskCache`]（500 MB、LRU）：
//! 命中则直接读本地字节解码，冷启动也能秒开；未命中才走网络，成功后回写磁盘。
//! 磁盘用量由后台周期任务刷新，`stats()` 只读原子量，不在渲染路径遍历目录。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::*;

use siren_core::network::client::NeteaseClient;

use super::cover_disk::{DiskCache, DiskStats};
use super::{cover_placeholder, cover_placeholder_round};

/// 缓存总上限：压缩字节 + 解码位图估算。
const CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 淘汰水位：一次清到该值以下，避免刚好卡在阈值反复触发。
const CACHE_LOW_WATER: u64 = 48 * 1024 * 1024;
/// 条目最短驻留时间：刚插入的条目即使超限也先留一会儿。
const EVICT_MIN_AGE: Duration = Duration::from_secs(10);
/// 主色最多保留条数（每条约 100 B，但仍要有上限）。
const COLOR_MAX_ENTRIES: usize = 512;
/// 低于该档不取主色：小图取色偏差大，且低档多用于列表/头像。
const COLOR_MIN_BUCKET: u32 = 128;
/// 交互预取（页面/切歌）的并发上限。
const PREFETCH_CONCURRENCY: usize = 6;
/// 批量预取（整条队列）的并发上限：单独一把额度，避免长队列把交互封面的
/// 连接全占住（tokio 信号量是 FIFO，共用一把会让页面封面排在几千个队列封面之后）。
const BULK_PREFETCH_CONCURRENCY: usize = 2;

/// 尺寸档位：向上取到最近的档（≥ 请求像素）。
const BUCKETS: [u32; 5] = [64, 128, 256, 512, 1024];

/// 逻辑像素（不含 DPR）→ 档位。
fn bucket(size: f32) -> u32 {
    let size = size.max(1.0) as u32;
    *BUCKETS
        .iter()
        .find(|&&b| b >= size)
        .unwrap_or(&BUCKETS[BUCKETS.len() - 1])
}

/// 请求档位 = 逻辑尺寸 × DPR，再向上取档。
fn request_bucket(size: f32, dpr: f32) -> u32 {
    bucket(size * dpr)
}

/// 档位回退顺序：优先更大的档（缩小比放大清晰），再更小的。
fn fallback_buckets(exact: u32) -> impl Iterator<Item = u32> {
    BUCKETS.iter().rev().copied().filter(move |b| *b != exact)
}

/// 单条成本估算：解码位图（精确 `W×H×4`）+ 压缩字节。
fn entry_cost(entry: &Entry) -> u64 {
    let decoded = u64::from(entry.bucket) * u64::from(entry.bucket) * 4;
    decoded + entry.image.bytes.len() as u64
}

/// 缓存中的一条。
struct Entry {
    image: Arc<Image>,
    bucket: u32,
    /// 最近一次被渲染/预取碰到的时刻（淘汰依据）。
    last_seen: Instant,
}

/// 缓存占用快照（设置页展示与周期日志）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoverStats {
    pub entries: usize,
    pub compressed_bytes: u64,
    pub decoded_bytes: u64,
    /// 磁盘缓存（封面/头像的本地字节）。
    pub disk: DiskStats,
}

impl CoverStats {
    pub(crate) fn total_bytes(&self) -> u64 {
        self.compressed_bytes + self.decoded_bytes
    }

    /// 约多少 MB（展示用，保留一位小数由调用方处理）。
    pub(crate) fn total_mb(&self) -> f64 {
        self.total_bytes() as f64 / (1024.0 * 1024.0)
    }
}

/// 主色条目：（颜色, 来源档位, 最近使用）。
type ColorEntry = (Rgba, u32, Instant);
/// 主色表：base URL → 条目。
type ColorMap = Arc<Mutex<HashMap<String, ColorEntry>>>;

#[derive(Clone)]
pub(crate) struct CoverLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
    /// 主色：base URL → （颜色, 来源档位, 最近使用）。同封面取最大档的颜色。
    colors: ColorMap,
    inflight: Arc<Mutex<HashSet<String>>>,
    /// 当前 DPR（f32 位模式）。由 `Root::render` 每帧更新。
    dpr: Arc<AtomicU32>,
    /// 代数：`clear()` 自增，旧任务的回写因此失效。
    gen: Arc<AtomicU64>,
    sem: Arc<tokio::sync::Semaphore>,
    bulk_sem: Arc<tokio::sync::Semaphore>,
    /// 磁盘层（无缓存目录时为 None，功能降级为纯内存）。
    disk: Option<Arc<DiskCache>>,
    /// 磁盘用量快照：由周期任务刷新，`stats()` 只读（渲染路径不遍历目录）。
    disk_bytes: Arc<AtomicU64>,
    disk_files: Arc<AtomicUsize>,
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
}

/// 去 query 的基准 URL（主色表的键）。
fn base_url(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_string()
}

impl CoverLoader {
    pub(crate) fn new(client: Arc<NeteaseClient>, handle: tokio::runtime::Handle) -> Self {
        let disk = DiskCache::open_default().map(Arc::new);
        if let Some(cache) = &disk {
            log::info!("封面磁盘缓存：{}", cache.dir().display());
        } else {
            log::warn!("封面磁盘缓存不可用（取不到缓存目录），仅用内存缓存");
        }
        let disk_bytes = Arc::new(AtomicU64::new(0));
        let disk_files = Arc::new(AtomicUsize::new(0));
        // 启动即后台 trim 一次（对齐 Melodia 的启动清理），之后每 60s 刷新用量。
        if let Some(cache) = disk.clone() {
            let bytes = disk_bytes.clone();
            let files = disk_files.clone();
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
                        "封面磁盘缓存：{} 个文件 / {:.1} MB（上限 {} MB）",
                        stats.files,
                        stats.bytes as f64 / (1024.0 * 1024.0),
                        super::cover_disk::DISK_LIMIT_BYTES / (1024 * 1024)
                    );
                    tick.tick().await;
                }
            });
        }
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            colors: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            dpr: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            gen: Arc::new(AtomicU64::new(0)),
            sem: Arc::new(tokio::sync::Semaphore::new(PREFETCH_CONCURRENCY)),
            bulk_sem: Arc::new(tokio::sync::Semaphore::new(BULK_PREFETCH_CONCURRENCY)),
            disk,
            disk_bytes,
            disk_files,
            client,
            handle,
        }
    }

    /// 磁盘用量快照（原子量，渲染路径可安全调用）。
    fn disk_stats(&self) -> DiskStats {
        DiskStats {
            bytes: self.disk_bytes.load(Ordering::Relaxed),
            files: self.disk_files.load(Ordering::Relaxed),
        }
    }

    /// 写入当前 DPR（2× 屏上请求尺寸翻倍）。非有限值回落 1.0。
    pub(crate) fn set_dpr(&self, dpr: f32) {
        let dpr = if dpr.is_finite() && dpr > 0.0 {
            dpr.clamp(0.5, 4.0)
        } else {
            1.0
        };
        self.dpr.store(dpr.to_bits(), Ordering::Relaxed);
    }

    fn dpr(&self) -> f32 {
        let dpr = f32::from_bits(self.dpr.load(Ordering::Relaxed));
        if dpr.is_finite() && dpr > 0.0 {
            dpr
        } else {
            1.0
        }
    }

    /// 只读查表：先精确档，再按「大档优先」回退。
    ///
    /// 回退是纯读操作（不发请求），因此档位写错时最坏是「糊一点」，
    /// 而不是永远灰色占位。
    fn lookup(&self, url: &str, size: f32) -> Option<Arc<Image>> {
        let exact = request_bucket(size, self.dpr());
        let mut cache = self.cache.lock().unwrap();
        let key = sized_url(url, exact);
        if let Some(entry) = cache.get_mut(&key) {
            entry.last_seen = Instant::now();
            return Some(entry.image.clone());
        }
        for b in fallback_buckets(exact) {
            let key = sized_url(url, b);
            if let Some(entry) = cache.get_mut(&key) {
                entry.last_seen = Instant::now();
                log::trace!("封面档位回退：请求 {exact} 命中 {b}（{url}）");
                return Some(entry.image.clone());
            }
        }
        log::trace!("封面未命中：bucket={exact}（{url}）");
        None
    }

    /// 只读：命中缓存则渲染图片，否则返回占位。
    pub(crate) fn image(&self, url: &str, size: f32) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder(size).into_any_element();
        }
        match self.lookup(url, size) {
            Some(image) => img(image)
                .w(px(size))
                .h(px(size))
                .object_fit(ObjectFit::Cover)
                .rounded(px(6.0))
                .into_any_element(),
            None => cover_placeholder(size).into_any_element(),
        }
    }

    /// 只读：取封面图片（供拖动预览等每帧自建元素的场景复用）。
    pub(crate) fn image_arc(&self, url: &str, size: f32) -> Option<Arc<Image>> {
        if url.is_empty() {
            return None;
        }
        self.lookup(url, size)
    }

    /// 封面主色（未取到返回 None）。
    pub(crate) fn color(&self, url: &str) -> Option<Rgba> {
        let base = base_url(url);
        let mut colors = self.colors.lock().unwrap();
        colors.get_mut(&base).map(|(color, _, seen)| {
            *seen = Instant::now();
            *color
        })
    }

    /// 只读：圆形裁剪（头像）。
    pub(crate) fn image_round(&self, url: &str, size: f32) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder_round(size);
        }
        match self.lookup(url, size) {
            Some(image) => img(image)
                .w(px(size))
                .h(px(size))
                .object_fit(ObjectFit::Cover)
                .rounded_full()
                .into_any_element(),
            None => cover_placeholder_round(size),
        }
    }

    /// 预取一批封面（数据加载完成后调用）；完成后通知重绘。
    ///
    /// 会在 UI 线程先做一次淘汰（`remove_asset` 需要 `&mut App`）。
    pub(crate) fn prefetch<T: 'static>(&self, urls: impl IntoIterator<Item = String>, size: f32, cx: &mut Context<T>) {
        let sem = self.sem.clone();
        self.prefetch_with(urls, size, cx, sem);
    }

    /// 批量预取（整条队列）：用独立的低并发额度，避免饿死交互封面。
    pub(crate) fn prefetch_bulk<T: 'static>(
        &self,
        urls: impl IntoIterator<Item = String>,
        size: f32,
        cx: &mut Context<T>,
    ) {
        let sem = self.bulk_sem.clone();
        self.prefetch_with(urls, size, cx, sem);
    }

    fn prefetch_with<T: 'static>(
        &self,
        urls: impl IntoIterator<Item = String>,
        size: f32,
        cx: &mut Context<T>,
        sem: Arc<tokio::sync::Semaphore>,
    ) {
        self.evict(&mut *cx);
        let req = request_bucket(size, self.dpr());
        let gen = self.gen.load(Ordering::Relaxed);
        // 同一批里可能有重复 URL（队列/列表），只抓一次。
        let mut seen = HashSet::new();
        for url in urls {
            if url.is_empty() {
                continue;
            }
            let key = sized_url(&url, req);
            if !seen.insert(key.clone()) {
                continue;
            }
            if self.cache.lock().unwrap().contains_key(&key) {
                continue;
            }
            if !self.inflight.lock().unwrap().insert(key.clone()) {
                continue;
            }

            // 抓取、写缓存、清 inflight 全在 tokio 任务里完成：不依赖发起方的页面实体存活，
            // 页面先销毁导致通知任务被取消时也不会把键永久留在 inflight（否则该封面永不加载）。
            let client = self.client.clone();
            let cache = self.cache.clone();
            let colors = self.colors.clone();
            let inflight = self.inflight.clone();
            let sem = sem.clone();
            let gen_arc = self.gen.clone();
            let disk = self.disk.clone();
            let disk_bytes = self.disk_bytes.clone();
            let disk_files = self.disk_files.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let base = base_url(&url);
            self.handle.spawn(async move {
                // 并发上限：批量预取（如打开大队列）排队进行，不一次打满连接。
                let Ok(_permit) = sem.acquire().await else {
                    // 信号量只会在关闭时失败（当前不会发生）；仍要清 inflight 并唤醒等待方，
                    // 否则该键永久留在 inflight，对应封面再也抓不到。
                    inflight.lock().unwrap().remove(&key);
                    let _ = tx.send(());
                    return;
                };
                // ① 磁盘命中：读本地字节，冷启动不必重新下载。读盘放 blocking 线程池。
                let from_disk = match disk.clone() {
                    Some(cache) => {
                        let key = key.clone();
                        tokio::task::spawn_blocking(move || cache.get(&key))
                            .await
                            .ok()
                            .flatten()
                    }
                    None => None,
                };
                let hits_disk = from_disk.is_some();
                if hits_disk {
                    log::debug!("封面磁盘命中：{key}");
                }
                let fetched = match from_disk {
                    Some(bytes) => Some(bytes),
                    None => client.get_bytes(&key).await.ok(),
                };
                if let Some(bytes) = fetched {
                    // ② 网络取到的新字节回写磁盘（命中的不重复写，省一次 IO）。
                    if !hits_disk {
                        if let Some(cache) = disk.clone() {
                            let key = key.clone();
                            let len = bytes.len() as u64;
                            let payload = bytes.clone();
                            let written = tokio::task::spawn_blocking(move || {
                                cache.put(&key, &payload);
                            })
                            .await;
                            if written.is_ok() {
                                disk_bytes.fetch_add(len, Ordering::Relaxed);
                                disk_files.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    // 解码 / 取主色是 CPU 密集，放到 blocking 线程池，避免拖慢同 runtime 的网络请求。
                    let computed = tokio::task::spawn_blocking(move || {
                        let color = if req >= COLOR_MIN_BUCKET {
                            dominant_color(&bytes)
                        } else {
                            None
                        };
                        let image = guess_format(&bytes).map(|fmt| Arc::new(Image::from_bytes(fmt, bytes)));
                        (color, image)
                    })
                    .await;
                    if let Ok((color, image)) = computed {
                        // 清空缓存后（gen 变化）旧任务不得回写。
                        if gen_arc.load(Ordering::Relaxed) == gen {
                            if let Some(color) = color {
                                let mut map = colors.lock().unwrap();
                                // 同一封面取「最大档」的颜色：与到达顺序无关。
                                match map.get_mut(&base) {
                                    Some((stored, stored_bucket, _)) if req > *stored_bucket => {
                                        *stored = color;
                                        *stored_bucket = req;
                                    }
                                    Some(_) => {}
                                    None => {
                                        map.insert(base, (color, req, Instant::now()));
                                    }
                                }
                            }
                            if let Some(image) = image {
                                let mut cache = cache.lock().unwrap();
                                // 持锁复查：clear() 先自增 gen 再 drain，若本任务在
                                // 自增前拿到锁、自增后插入，这里是最后一道拦截。
                                if gen_arc.load(Ordering::Relaxed) == gen {
                                    cache.insert(
                                        key.clone(),
                                        Entry {
                                            image,
                                            bucket: req,
                                            last_seen: Instant::now(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                inflight.lock().unwrap().remove(&key);
                let _ = tx.send(());
            });

            // 仅用于完成后通知发起方重绘（发起方可能已销毁，忽略即可）。
            cx.spawn(async move |this, cx| {
                let _ = rx.await;
                this.update(cx, |_this, cx| cx.notify()).ok();
            })
            .detach();
        }
    }

    /// 淘汰到水位以下，并同步释放 GPUI 侧的解码副本。
    ///
    /// 必须由 UI 线程调用（`remove_asset` 需要 `&mut App`）。只淘汰「至少
    /// [`EVICT_MIN_AGE`] 没被碰过」的条目：屏上正在渲染的图每帧刷新 `last_seen`，
    /// 否则删掉后下一帧又会解码插入，形成抖动。
    pub(crate) fn evict(&self, cx: &mut App) {
        let mut removed: Vec<Arc<Image>> = Vec::new();
        let live_ids: HashSet<u64>;
        {
            let mut cache = self.cache.lock().unwrap();
            let mut total: u64 = cache.values().map(entry_cost).sum();
            if total > CACHE_MAX_BYTES {
                let now = Instant::now();
                let mut candidates: Vec<(String, Instant, u64)> = cache
                    .iter()
                    .filter(|(_, e)| now.duration_since(e.last_seen) >= EVICT_MIN_AGE)
                    .map(|(k, e)| (k.clone(), e.last_seen, entry_cost(e)))
                    .collect();
                candidates.sort_by_key(|(_, seen, _)| *seen);
                for (key, _, cost) in candidates {
                    if total <= CACHE_LOW_WATER {
                        break;
                    }
                    if let Some(entry) = cache.remove(&key) {
                        total = total.saturating_sub(cost);
                        removed.push(entry.image);
                    }
                }
            }
            live_ids = cache.values().map(|e| e.image.id).collect();
        }
        // 同内容（image.id 相同）可能被多条 URL 引用，仍被引用时不能删。
        let mut dropped = HashSet::new();
        for image in removed {
            if live_ids.contains(&image.id) || !dropped.insert(image.id) {
                continue;
            }
            ImageSource::Image(image).remove_asset(cx);
        }
        self.prune_colors();
    }

    /// 主色表按条数封顶（条目很小，不需要按字节算）。
    fn prune_colors(&self) {
        let mut colors = self.colors.lock().unwrap();
        if colors.len() <= COLOR_MAX_ENTRIES {
            return;
        }
        let mut items: Vec<(String, Instant)> = colors.iter().map(|(k, (_, _, seen))| (k.clone(), *seen)).collect();
        items.sort_by_key(|(_, seen)| *seen);
        let remove_n = colors.len() - COLOR_MAX_ENTRIES;
        for (key, _) in items.into_iter().take(remove_n) {
            colors.remove(&key);
        }
    }

    /// 清空全部缓存（设置页「清除图片缓存」）：内存 + 磁盘。
    pub(crate) fn clear(&self, cx: &mut App) {
        // 旧任务据此丢弃回写；inflight 一并清空。
        self.gen.fetch_add(1, Ordering::Relaxed);
        let images: Vec<Arc<Image>> = {
            let mut cache = self.cache.lock().unwrap();
            cache.drain().map(|(_, entry)| entry.image).collect()
        };
        self.colors.lock().unwrap().clear();
        self.inflight.lock().unwrap().clear();
        if let Some(disk) = self.disk.clone() {
            // 删文件放 blocking 线程池，避免设置页点一下卡住 UI。
            self.handle.spawn(async move {
                let removed = tokio::task::spawn_blocking(move || disk.clear()).await.unwrap_or(0);
                log::info!("已清除封面磁盘缓存：{removed} 个文件");
            });
        }
        self.disk_bytes.store(0, Ordering::Relaxed);
        self.disk_files.store(0, Ordering::Relaxed);
        let mut seen = HashSet::new();
        for image in images {
            if seen.insert(image.id) {
                ImageSource::Image(image).remove_asset(cx);
            }
        }
    }

    /// 当前占用快照。
    pub(crate) fn stats(&self) -> CoverStats {
        let cache = self.cache.lock().unwrap();
        let mut stats = CoverStats {
            entries: cache.len(),
            disk: self.disk_stats(),
            ..Default::default()
        };
        for entry in cache.values() {
            stats.compressed_bytes += entry.image.bytes.len() as u64;
            stats.decoded_bytes += u64::from(entry.bucket) * u64::from(entry.bucket) * 4;
        }
        stats
    }
}

/// 网易云封面支持 `?param=WxH` 生成指定尺寸，减少下载量。
fn sized_url(url: &str, size: u32) -> String {
    if url.contains("126.net") && !url.contains("param=") {
        // URL 可能已带其它 query，此时用 `&` 追加，避免拼出两个 `?` 的非法地址。
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}param={size}y{size}")
    } else {
        url.to_string()
    }
}

/// 提取封面主色：缩小后取饱和度加权的平均色。
fn dominant_color(bytes: &[u8]) -> Option<Rgba> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img.resize_exact(8, 8, image::imageops::FilterType::Triangle).to_rgb8();
    let mut rs = 0f32;
    let mut gs = 0f32;
    let mut bs = 0f32;
    let mut ws = 0f32;
    for px in small.pixels() {
        let [r, g, b] = px.0;
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let sat = if max <= 0.0 { 0.0 } else { (max - min) / max };
        let w = 0.2 + sat;
        rs += rf * w;
        gs += gf * w;
        bs += bf * w;
        ws += w;
    }
    if ws <= 0.0 {
        return None;
    }
    let (r, g, b) = ((rs / ws) as u8, (gs / ws) as u8, (bs / ws) as u8);
    Some(rgb((r as u32) << 16 | (g as u32) << 8 | b as u32))
}

/// 通过魔数猜测图片格式。
fn guess_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        Some(ImageFormat::Png)
    } else if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 6 && (&bytes[..6] == b"GIF89a" || &bytes[..6] == b"GIF87a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    // 不用 `use super::*`：会把父模块的 `gpui_kit::*` 带进来，
    // 展开 `#[test]` 时撞上默认递归深度上限（见 lib.rs 的 `recursion_limit`）。
    use std::sync::Arc;
    use std::time::Instant;

    use gpui_kit::{Image, ImageFormat};

    use super::{bucket, entry_cost, fallback_buckets, request_bucket, sized_url, CoverStats, Entry};

    #[test]
    fn sized_url_appends_param_with_correct_separator() {
        assert_eq!(
            sized_url("https://p1.music.126.net/x.jpg", 64),
            "https://p1.music.126.net/x.jpg?param=64y64"
        );
        // 已带其它 query 时用 `&`，不产生第二个 `?`。
        assert_eq!(
            sized_url("https://p1.music.126.net/x.jpg?foo=1", 64),
            "https://p1.music.126.net/x.jpg?foo=1&param=64y64"
        );
        // 已带 param 的 URL 原样返回。
        assert_eq!(
            sized_url("https://p1.music.126.net/x.jpg?param=100y100", 64),
            "https://p1.music.126.net/x.jpg?param=100y100"
        );
        // 非 126.net 不处理。
        assert_eq!(sized_url("https://example.com/x.jpg", 64), "https://example.com/x.jpg");
    }

    #[test]
    fn bucket_groups_nearby_sizes() {
        assert_eq!(bucket(28.0), 64);
        assert_eq!(bucket(32.0), 64);
        assert_eq!(bucket(36.0), 64);
        assert_eq!(bucket(40.0), 64);
        assert_eq!(bucket(48.0), 64);
        assert_eq!(bucket(64.0), 64);
        assert_eq!(bucket(65.0), 128);
        assert_eq!(bucket(120.0), 128);
        assert_eq!(bucket(220.0), 256);
        assert_eq!(bucket(9999.0), 1024);
    }

    #[test]
    fn dpr_scales_the_request_bucket() {
        // 2× 屏上 40px 的行需要 80 物理像素 → 128 档，而不是 64。
        assert_eq!(request_bucket(40.0, 1.0), 64);
        assert_eq!(request_bucket(40.0, 2.0), 128);
        // NP 大封面：1× 时 356 → 512，2× 时 712 → 1024。
        assert_eq!(request_bucket(356.0, 1.0), 512);
        assert_eq!(request_bucket(356.0, 2.0), 1024);
        // 非法 DPR 不应放大尺寸：NaN 参与比较时按 1px 归一，落到最小档。
        // （真正的入口 `set_dpr` 已把非有限值夹成 1.0，这里只兜底。）
        assert_eq!(request_bucket(40.0, f32::NAN), 64);
    }

    #[test]
    fn fallback_prefers_larger_buckets_and_never_repeats_exact() {
        let order: Vec<u32> = fallback_buckets(256).collect();
        assert_eq!(order, vec![1024, 512, 128, 64]);
        assert!(!order.contains(&256));
    }

    #[test]
    fn entry_cost_counts_decoded_bitmap_and_bytes() {
        // 用一个最小 Image 构造（bytes 长度可控）。
        let image = Arc::new(Image {
            format: ImageFormat::Png,
            bytes: vec![0; 1000],
            id: 7,
        });
        let entry = Entry {
            image,
            bucket: 512,
            last_seen: Instant::now(),
        };
        assert_eq!(entry_cost(&entry), 512 * 512 * 4 + 1000);
    }

    #[test]
    fn stats_reports_both_layers() {
        let stats = CoverStats {
            entries: 2,
            compressed_bytes: 100,
            decoded_bytes: 4096,
            disk: Default::default(),
        };
        assert_eq!(stats.total_bytes(), 4196);
        assert!(stats.total_mb() > 0.0);
    }
}
