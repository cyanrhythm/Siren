//! 封面加载器：按尺寸缓存 + 后台抓取 + 完成后通知重绘。
//!
//! 当前由列表项渲染时按需触发抓取（命中缓存则直接读）。后续可改为在数据加载
//! 完成时预取，避免在 render 路径中发起网络请求（见 `docs` 与 coding-guides）。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gpui_kit::*;

use siren_core::network::client::NeteaseClient;

use super::{cover_placeholder, Root};

#[derive(Clone)]
pub(crate) struct CoverLoader {
    cache: Arc<Mutex<HashMap<String, Arc<Image>>>>,
    inflight: Arc<Mutex<HashSet<String>>>,
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
}

impl CoverLoader {
    pub(crate) fn new(client: Arc<NeteaseClient>, handle: tokio::runtime::Handle) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            client,
            handle,
        }
    }

    pub(crate) fn image(&self, url: &str, size: f32, weak: &WeakEntity<Root>, cx: &mut App) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder(size).into_any_element();
        }
        let key = sized_url(url, size);
        if let Some(image) = self.cache.lock().unwrap().get(&key).cloned() {
            return img(image).w(px(size)).h(px(size)).rounded(px(6.0)).into_any_element();
        }
        self.request(key, weak, cx);
        cover_placeholder(size).into_any_element()
    }

    fn request(&self, key: String, weak: &WeakEntity<Root>, cx: &mut App) {
        if !self.inflight.lock().unwrap().insert(key.clone()) {
            return;
        }
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<Option<Arc<Image>>>();
        let fetch_key = key.clone();
        self.handle.spawn(async move {
            let image = match client.get_bytes(&fetch_key).await {
                Ok(bytes) => guess_format(&bytes).map(|fmt| Arc::new(Image::from_bytes(fmt, bytes))),
                Err(_) => None,
            };
            let _ = tx.send(image);
        });

        let cache = self.cache.clone();
        let inflight = self.inflight.clone();
        let key_for = key;
        weak.clone()
            .update(cx, move |_this, cx| {
                cx.spawn(async move |this, cx| {
                    let image = rx.await.ok().flatten();
                    inflight.lock().unwrap().remove(&key_for);
                    if let Some(image) = image {
                        cache.lock().unwrap().insert(key_for, image);
                        this.update(cx, |_this, cx| cx.notify()).ok();
                    }
                })
                .detach();
            })
            .ok();
    }
}

/// 网易云封面支持 `?param=WxH` 生成指定尺寸，减少下载量。
fn sized_url(url: &str, size: f32) -> String {
    if url.contains("126.net") && !url.contains("param=") {
        format!("{url}?param={}y{}", size as u32, size as u32)
    } else {
        url.to_string()
    }
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
