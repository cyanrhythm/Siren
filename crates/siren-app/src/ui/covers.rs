//! 封面加载器：按尺寸分桶缓存。
//!
//! 抓取由 [`CoverLoader::prefetch`] 在**数据加载完成时**触发；
//! [`CoverLoader::image`] 仅做只读查询（命中缓存渲染图片，否则占位），
//! 不在 render 路径中发起网络请求（对齐 coding-guides 的副作用规则）。
//!
//! 缓存键使用 [`bucket`] 归一后的尺寸：请求尺寸先向上取到 64/128/256/512 档，
//! 使 28/32/36 等相近尺寸共用同一张图，避免“预取与渲染尺寸不一致导致缓存永不命中”。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gpui_kit::*;

use siren_core::network::client::NeteaseClient;

use super::{cover_placeholder, cover_placeholder_round};

/// 尺寸分档：向上取到最近的档位（≥ 请求尺寸）。
fn bucket(size: f32) -> u32 {
    const BUCKETS: [u32; 4] = [64, 128, 256, 512];
    let size = size.max(1.0) as u32;
    *BUCKETS
        .iter()
        .find(|&&b| b >= size)
        .unwrap_or(&BUCKETS[BUCKETS.len() - 1])
}

#[derive(Clone)]
pub(crate) struct CoverLoader {
    cache: Arc<Mutex<HashMap<String, Arc<Image>>>>,
    colors: Arc<Mutex<HashMap<String, Rgba>>>,
    inflight: Arc<Mutex<HashSet<String>>>,
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
}

impl CoverLoader {
    pub(crate) fn new(client: Arc<NeteaseClient>, handle: tokio::runtime::Handle) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            colors: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            client,
            handle,
        }
    }

    /// 只读：命中缓存则渲染图片，否则返回占位。
    pub(crate) fn image(&self, url: &str, size: f32) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder(size).into_any_element();
        }
        let key = sized_url(url, bucket(size));
        if let Some(image) = self.cache.lock().unwrap().get(&key).cloned() {
            return img(image).w(px(size)).h(px(size)).rounded(px(6.0)).into_any_element();
        }
        cover_placeholder(size).into_any_element()
    }

    /// 封面主色（未取到返回 None）。
    pub(crate) fn color(&self, url: &str) -> Option<Rgba> {
        let base = url.split('?').next().unwrap_or(url).to_string();
        self.colors.lock().unwrap().get(&base).copied()
    }

    /// 只读：圆形裁剪（头像）。
    pub(crate) fn image_round(&self, url: &str, size: f32) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder_round(size);
        }
        let key = sized_url(url, bucket(size));
        if let Some(image) = self.cache.lock().unwrap().get(&key).cloned() {
            return img(image).w(px(size)).h(px(size)).rounded_full().into_any_element();
        }
        cover_placeholder_round(size)
    }

    /// 预取一批封面（数据加载完成后调用）；完成后通知重绘。
    pub(crate) fn prefetch<T: 'static>(&self, urls: impl IntoIterator<Item = String>, size: f32, cx: &mut Context<T>) {
        let size = bucket(size);
        for url in urls {
            if url.is_empty() {
                continue;
            }
            let key = sized_url(&url, size);
            if self.cache.lock().unwrap().contains_key(&key) {
                continue;
            }
            if !self.inflight.lock().unwrap().insert(key.clone()) {
                continue;
            }

            let client = self.client.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<(Option<Arc<Image>>, Option<Rgba>)>();
            let fetch_key = key.clone();
            let base = url.split('?').next().unwrap_or(&url).to_string();
            self.handle.spawn(async move {
                match client.get_bytes(&fetch_key).await {
                    Ok(bytes) => {
                        let color = dominant_color(&bytes);
                        let image = guess_format(&bytes).map(|fmt| Arc::new(Image::from_bytes(fmt, bytes)));
                        let _ = tx.send((image, color));
                    }
                    Err(_) => {
                        let _ = tx.send((None, None));
                    }
                }
            });

            let cache = self.cache.clone();
            let colors = self.colors.clone();
            let inflight = self.inflight.clone();
            cx.spawn(async move |this, cx| {
                let (image, color) = rx.await.unwrap_or((None, None));
                inflight.lock().unwrap().remove(&key);
                if let Some(color) = color {
                    colors.lock().unwrap().insert(base, color);
                }
                if let Some(image) = image {
                    cache.lock().unwrap().insert(key, image);
                    this.update(cx, |_this, cx| cx.notify()).ok();
                }
            })
            .detach();
        }
    }
}

/// 网易云封面支持 `?param=WxH` 生成指定尺寸，减少下载量。
fn sized_url(url: &str, size: u32) -> String {
    if url.contains("126.net") && !url.contains("param=") {
        format!("{url}?param={size}y{size}")
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
    #[test]
    fn bucket_groups_nearby_sizes() {
        assert_eq!(super::bucket(28.0), 64);
        assert_eq!(super::bucket(32.0), 64);
        assert_eq!(super::bucket(36.0), 64);
        assert_eq!(super::bucket(40.0), 64);
        assert_eq!(super::bucket(48.0), 64);
        assert_eq!(super::bucket(64.0), 64);
        assert_eq!(super::bucket(65.0), 128);
        assert_eq!(super::bucket(120.0), 128);
        assert_eq!(super::bucket(220.0), 256);
        assert_eq!(super::bucket(9999.0), 512);
    }
}
