//! 歌单详情：大封面头部 + 操作栏 + 曲目表（对齐 Melodia `PlaylistPage.kt`）。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::{transition, StyledExt, Transition};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{PlaylistDetail, Song};

use super::super::components::like_button;
use super::super::motion;
use super::super::playback::Playback;
use super::super::services::Services;
use super::super::smooth_scroll::{smooth_scroll, smooth_scroll_offset, smooth_virtual_list};
use super::super::{fmt_time, icon, page_shell_with_header, PageEvent};
use super::dim;
use crate::theme::{self, spacing};
use siren_core::player::{PlaySource, PlaySourceKind};

/// 顶部 hero 的固定高度：`p(XL) * 2 + 封面 200`（收起动画按它插值）。
const HERO_H: f32 = 248.0;
/// 收起后的紧凑横栏高度，对齐 Melodia `StickyBarHeight = 64.dp`。
const STICKY_BAR_H: f32 = 64.0;
/// 操作栏高度：播放键 56 + `pb(10)`。
const ACTION_ROW_H: f32 = 66.0;
/// 自建歌单「添加 / 名称和详情」胶囊行高度：pill 32 + `pb(14)`。
const PILLS_H: f32 = 46.0;
/// 曲目区滚动超过该偏移即收起 hero（滚回顶部再展开）。
const COLLAPSE_AT_PX: f32 = 8.0;

/// 封面裁剪状态。
struct CropState {
    orig: Arc<Vec<u8>>,
    image: Arc<Image>,
    disp_w: f32,
    disp_h: f32,
    src_w: f32,
    src_h: f32,
    /// 选择方框（展示坐标）：(x, y, 边长)
    rect: (f32, f32, f32),
    drag_start: Option<(f32, f32)>,
    /// 展示区左上角的窗口坐标（canvas 回填）。
    origin: Rc<Cell<(f32, f32)>>,
}

/// 曲目排序。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Custom,
    Name,
    Duration,
}

impl SortOrder {
    fn label(self) -> &'static str {
        match self {
            SortOrder::Custom => "自定义顺序",
            SortOrder::Name => "按名称",
            SortOrder::Duration => "按时长",
        }
    }
    fn next(self) -> SortOrder {
        match self {
            SortOrder::Custom => SortOrder::Name,
            SortOrder::Name => SortOrder::Duration,
            SortOrder::Duration => SortOrder::Custom,
        }
    }
}

pub(crate) struct PlaylistPage {
    services: Services,
    playback: Entity<Playback>,
    id: i64,
    name: String,
    detail: Option<Arc<PlaylistDetail>>,
    /// 加载失败：用于展示错误态与重试（而非永久停在“加载中”）。
    failed: bool,
    subscribed: bool,
    query_input: Entity<InputState>,
    query: String,
    sort: SortOrder,
    /// 当前登录用户 uid（用于判断是否本人创建）。
    my_uid: Option<i64>,
    /// 「添加歌曲」搜索框与结果。
    add_input: Entity<InputState>,
    add_open: bool,
    add_results: Vec<Song>,
    /// 「添加歌曲」搜索代次：连按回车时旧结果据此失效（J7）。
    add_gen: u64,
    /// 封面裁剪浮层状态。
    crop: Option<CropState>,
    /// 加载代：重载时自增，旧响应据此失效。
    load_gen: u64,
    /// 「从歌单移除」二次确认（歌曲 id + 歌名）。
    remove_track_confirm: Option<(i64, String)>,
    /// 「移除歌单」（取消收藏）二次确认。
    remove_playlist_confirm: bool,
    _filter_sub: Subscription,
    _add_sub: Subscription,
}

impl EventEmitter<PageEvent> for PlaylistPage {}

impl PlaylistPage {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        cx: &mut Context<Self>,
        services: Services,
        playback: Entity<Playback>,
        id: i64,
        name: String,
        query_input: Entity<InputState>,
        add_input: Entity<InputState>,
        my_uid: Option<i64>,
    ) -> Self {
        let _filter_sub = cx.subscribe(&query_input, |this: &mut PlaylistPage, _i, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                this.query = this.query_input.read(cx).value().to_string();
                cx.notify();
            }
        });
        let _add_sub = cx.subscribe(&add_input, |this: &mut PlaylistPage, _i, ev: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.run_add_search(cx);
            }
        });
        let mut page = Self {
            services,
            playback,
            id,
            name,
            detail: None,
            failed: false,
            subscribed: false,
            query_input,
            query: String::new(),
            sort: SortOrder::Custom,
            my_uid,
            add_input,
            add_open: false,
            add_results: Vec::new(),
            add_gen: 0,
            crop: None,
            load_gen: 0,
            remove_track_confirm: None,
            remove_playlist_confirm: false,
            _filter_sub,
            _add_sub,
        };
        page.load(cx);
        page
    }

    /// 队列来源容器（用于「正在播放」与打卡上报）。
    fn source(&self) -> PlaySource {
        PlaySource {
            kind: PlaySourceKind::Playlist,
            id: self.id,
            name: self.name.clone(),
            cover_url: self.detail.as_ref().map(|d| d.cover_url.clone()).unwrap_or_default(),
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        self.failed = false;
        let client = self.services.client.clone();
        let id = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let t_all = std::time::Instant::now();
            let t = std::time::Instant::now();
            let mut detail = siren_core::network::api::playlist_detail(&client, id)
                .await
                .ok()
                .flatten();
            log::info!("[playlist] id={id} playlist/detail 用时 {:?}", t.elapsed());
            // 曲目被截断时，按 trackIds 分批补全（对齐 Melodia PlaylistTrackPaging）
            if let Some(d) = &mut detail {
                if !d.track_ids.is_empty() && d.track_ids.len() > d.tracks.len() {
                    // 只补 detail 未包含的曲目，且分批并发拉取（对齐 Melodia PlaylistTrackPaging）。
                    let have: std::collections::HashSet<i64> = d.tracks.iter().map(|t| t.id).collect();
                    let missing: Vec<i64> = d.track_ids.iter().copied().filter(|id| !have.contains(id)).collect();
                    let batches: Vec<Vec<i64>> = missing.chunks(500).map(|c| c.to_vec()).collect();
                    log::info!(
                        "[playlist] id={id} track_ids={} tracks={} 待补 {} 首，分 {} 批并发（限并发 4）",
                        d.track_ids.len(),
                        d.tracks.len(),
                        missing.len(),
                        batches.len()
                    );
                    let t_page = std::time::Instant::now();
                    let mut all: Vec<Song> = Vec::with_capacity(missing.len());
                    let mut set = tokio::task::JoinSet::new();
                    let mut iter = batches.into_iter();
                    const LIMIT: usize = 4;
                    for _ in 0..LIMIT {
                        if let Some(chunk) = iter.next() {
                            spawn_song_batch(&mut set, &client, chunk);
                        }
                    }
                    while let Some(joined) = set.join_next().await {
                        match joined {
                            Ok((n, elapsed, Ok(mut songs))) => {
                                log::debug!("[playlist] id={id} 批 n={n} 返回 {} 用时 {:?}", songs.len(), elapsed);
                                all.append(&mut songs);
                            }
                            Ok((n, elapsed, Err(e))) => {
                                log::warn!("[playlist] id={id} 批 n={n} 失败：{e} 用时 {:?}", elapsed)
                            }
                            Err(e) => log::warn!("[playlist] id={id} 批任务失败：{e}"),
                        }
                        if let Some(chunk) = iter.next() {
                            spawn_song_batch(&mut set, &client, chunk);
                        }
                    }
                    log::debug!(
                        "[playlist] id={id} 分批补全完成：总用时 {:?}，累计 {} 首",
                        t_page.elapsed(),
                        all.len()
                    );
                    // 合并 detail 已含曲目 + 补拉结果，再按 track_ids 顺序重排。
                    let mut map: std::collections::HashMap<i64, Song> = d.tracks.drain(..).map(|s| (s.id, s)).collect();
                    map.extend(all.into_iter().map(|s| (s.id, s)));
                    d.tracks = d.track_ids.iter().filter_map(|id| map.remove(id)).collect();
                }
                log::info!(
                    "[playlist] id={id} 拉取完成：最终 {} 首，总用时 {:?}",
                    d.tracks.len(),
                    t_all.elapsed()
                );
            } else {
                log::warn!("[playlist] id={id} 加载失败或为空，用时 {:?}", t_all.elapsed());
            }
            let _ = tx.send(detail);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some(d) = &detail {
                    this.subscribed = d.subscribed;
                }
                // 三态：成功/失败（失败展示错误态 + 重试）；不再永久停在“加载中”。
                this.failed = detail.is_none();
                this.detail = detail.map(Arc::new);
                if let Some(d) = &this.detail {
                    let urls: Vec<String> = d.tracks.iter().map(|t| t.cover_url.clone()).collect();
                    log::info!("[playlist] id={} 预取封面 {} 张（40px）", this.id, urls.len());
                    this.services.covers.prefetch(urls, 40.0, cx);
                    this.services.covers.prefetch([d.cover_url.clone()], 200.0, cx);
                    if !d.creator_avatar.is_empty() {
                        this.services.covers.prefetch([d.creator_avatar.clone()], 24.0, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_subscribe(&mut self, cx: &mut Context<Self>) {
        let subscribe = !self.subscribed;
        self.subscribed = subscribe;
        cx.notify();
        let client = self.services.client.clone();
        let id = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_subscribe(&client, id, subscribe)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.services.toast(
                        if subscribe {
                            "已收藏歌单"
                        } else {
                            "已取消收藏"
                        },
                        cx,
                    );
                } else {
                    this.subscribed = !subscribe;
                    this.services.toast_error("收藏歌单失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 从当前歌单移除某首歌，成功后重新加载曲目。
    fn remove_track(&mut self, song_id: i64, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let pid = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_delete_tracks(&client, pid, &[song_id])
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.load(cx);
                } else {
                    this.services.toast_error("移除歌曲失败，请重试", cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 打开「从歌单移除」确认（命名歌曲 + 歌单）。
    fn ask_remove_track(&mut self, song_id: i64, name: String, cx: &mut Context<Self>) {
        self.remove_track_confirm = Some((song_id, name));
        cx.notify();
    }
    fn cancel_remove_track(&mut self, cx: &mut Context<Self>) {
        self.remove_track_confirm = None;
        cx.notify();
    }
    fn confirm_remove_track(&mut self, cx: &mut Context<Self>) {
        if let Some((song_id, _)) = self.remove_track_confirm.take() {
            self.remove_track(song_id, cx);
        }
    }

    /// 打开「移除歌单」（取消收藏）确认。
    fn ask_remove_playlist(&mut self, cx: &mut Context<Self>) {
        self.remove_playlist_confirm = true;
        cx.notify();
    }
    fn cancel_remove_playlist(&mut self, cx: &mut Context<Self>) {
        self.remove_playlist_confirm = false;
        cx.notify();
    }
    fn confirm_remove_playlist(&mut self, cx: &mut Context<Self>) {
        self.remove_playlist_confirm = false;
        self.remove_playlist(cx);
    }

    /// 「从歌单移除」确认浮层。
    fn remove_track_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let Some((_, song_name)) = &self.remove_track_confirm else {
            return div().into_any_element();
        };
        let song_name = song_name.clone();
        let weak_cancel = weak.clone();
        let weak_ok = weak;
        let playlist_name = self.name.clone();
        div()
            .id("remove-track-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::scrim())
            .child(
                div()
                    .v_flex()
                    .w(px(360.0))
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(spacing::XL))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child(format!("从「{playlist_name}」移除\"{song_name}\"？")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(spacing::SM))
                            .child(
                                Button::new("remove-track-cancel")
                                    .label("取消")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_cancel.update(cx, |page, cx| page.cancel_remove_track(cx)).ok();
                                    }),
                            )
                            .child(danger_button("remove-track-ok", "移除", move |_ev, _w, cx| {
                                weak_ok.update(cx, |page, cx| page.confirm_remove_track(cx)).ok();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 「移除歌单」确认浮层。
    fn remove_playlist_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        if !self.remove_playlist_confirm {
            return div().into_any_element();
        }
        let weak_cancel = weak.clone();
        let weak_ok = weak;
        let playlist_name = self.name.clone();
        div()
            .id("remove-playlist-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::scrim())
            .child(
                div()
                    .v_flex()
                    .w(px(360.0))
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(spacing::XL))
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child(format!("从音乐库移除\"{playlist_name}\"？")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child("将取消收藏该歌单。"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(spacing::SM))
                            .child(
                                Button::new("remove-pl-cancel")
                                    .label("取消")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_cancel.update(cx, |page, cx| page.cancel_remove_playlist(cx)).ok();
                                    }),
                            )
                            .child(danger_button("remove-pl-ok", "移除", move |_ev, _w, cx| {
                                weak_ok.update(cx, |page, cx| page.confirm_remove_playlist(cx)).ok();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 更换封面：选择图片 → 打开裁剪浮层。
    fn change_cover(&mut self, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择封面图片".into()),
        };
        let task = cx.prompt_for_paths(options);
        cx.spawn(async move |this, cx| {
            let picked = match task.await {
                Ok(Ok(paths)) => paths.and_then(|mut v| v.pop()),
                _ => None,
            };
            let Some(path) = picked else { return };
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("jpg")
                .to_lowercase();
            let Ok(bytes) = std::fs::read(&path) else { return };
            let Ok(decoded) = image::load_from_memory(&bytes) else {
                return;
            };
            let (sw, sh) = (decoded.width() as f32, decoded.height() as f32);
            if sw <= 0.0 || sh <= 0.0 {
                return;
            }
            let max = 440.0_f32;
            let scale = (max / sw).min(max / sh);
            let disp_w = (sw * scale).max(1.0);
            let disp_h = (sh * scale).max(1.0);
            let side = disp_w.min(disp_h);
            let fmt = if ext == "png" {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            };
            let image = Arc::new(Image::from_bytes(fmt, bytes.clone()));
            this.update(cx, |page, cx| {
                page.crop = Some(CropState {
                    orig: Arc::new(bytes),
                    image,
                    disp_w,
                    disp_h,
                    src_w: sw,
                    src_h: sh,
                    rect: ((disp_w - side) / 2.0, (disp_h - side) / 2.0, side),
                    drag_start: None,
                    origin: Rc::new(Cell::new((0.0, 0.0))),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn crop_down(&mut self, pos: (f32, f32), cx: &mut Context<Self>) {
        if let Some(c) = &mut self.crop {
            let o = c.origin.get();
            let local = ((pos.0 - o.0).clamp(0.0, c.disp_w), (pos.1 - o.1).clamp(0.0, c.disp_h));
            c.drag_start = Some(local);
            c.rect = (local.0.min(c.disp_w - 1.0), local.1.min(c.disp_h - 1.0), 1.0);
            cx.notify();
        }
    }

    fn crop_move(&mut self, pos: (f32, f32), cx: &mut Context<Self>) {
        if let Some(c) = &mut self.crop {
            if let Some(start) = c.drag_start {
                let o = c.origin.get();
                let cur = ((pos.0 - o.0).clamp(0.0, c.disp_w), (pos.1 - o.1).clamp(0.0, c.disp_h));
                let side = (cur.0 - start.0)
                    .abs()
                    .max((cur.1 - start.1).abs())
                    .max(16.0)
                    .min(c.disp_w)
                    .min(c.disp_h);
                let x = start.0.min(cur.0).min(c.disp_w - side).max(0.0);
                let y = start.1.min(cur.1).min(c.disp_h - side).max(0.0);
                c.rect = (x, y, side);
                cx.notify();
            }
        }
    }

    fn crop_up(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = &mut self.crop {
            c.drag_start = None;
            cx.notify();
        }
    }

    fn crop_cancel(&mut self, cx: &mut Context<Self>) {
        self.crop = None;
        cx.notify();
    }

    /// 确认裁剪：按选区裁成正方形 → 缩放 → JPEG → NOS 上传登记。
    fn crop_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.crop.take() else { return };
        let k = c.src_w / c.disp_w;
        let sx = (c.rect.0 * k).round().max(0.0) as u32;
        let sy = (c.rect.1 * k).round().max(0.0) as u32;
        let src_w = c.src_w as u32;
        let src_h = c.src_h as u32;
        let side = ((c.rect.2 * k).round().max(1.0) as u32)
            .min(src_w.saturating_sub(sx))
            .min(src_h.saturating_sub(sy))
            .max(1);
        let client = self.services.client.clone();
        let handle = self.services.handle.clone();
        let pid = self.id;
        let orig = c.orig.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle.spawn(async move {
                let bytes = image::load_from_memory(&orig).ok().and_then(|img| {
                    let cropped = img.crop_imm(sx, sy, side, side);
                    let cropped = if side > 1024 {
                        cropped.resize(1024, 1024, image::imageops::FilterType::Lanczos3)
                    } else {
                        cropped
                    };
                    let rgb = cropped.to_rgb8();
                    let mut buf = Vec::new();
                    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
                    enc.encode_image(&rgb).ok().map(|_| buf)
                });
                let res = match bytes {
                    Some(bytes) => {
                        siren_core::network::api::update_playlist_cover(&client, pid, bytes, "jpg", "cover.jpg")
                            .await
                            .ok()
                    }
                    None => None,
                };
                let _ = tx.send(res);
            });
            let res = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if res.is_some() {
                    this.load(cx);
                    cx.emit(PageEvent::PlaylistChanged);
                } else {
                    this.services.toast_error("封面更新失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 裁剪浮层。
    fn crop_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let Some(c) = &self.crop else {
            return div().into_any_element();
        };
        let (rx, ry, rs) = c.rect;
        let (dw, dh) = (c.disp_w, c.disp_h);
        let origin_cell = c.origin.clone();

        let weak_down = weak.clone();
        let weak_move = weak.clone();
        let weak_up = weak.clone();
        let weak_cancel = weak.clone();
        let weak_ok = weak;

        let stage = div()
            .relative()
            .w(px(dw))
            .h(px(dh))
            .child(img(c.image.clone()).w(px(dw)).h(px(dh)))
            .child(
                div()
                    .absolute()
                    .left(px(rx))
                    .top(px(ry))
                    .w(px(rs))
                    .h(px(rs))
                    .border_2()
                    .border_color(theme::accent()),
            )
            .child(
                canvas(
                    move |bounds, _window, _cx| {
                        origin_cell.set((f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .w(px(dw))
                .h(px(dh)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w(px(dw))
                    .h(px(dh))
                    .on_mouse_down(MouseButton::Left, move |ev, _w, cx| {
                        let pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                        weak_down.update(cx, |page, cx| page.crop_down(pos, cx)).ok();
                    })
                    .on_mouse_move(move |ev, _w, cx| {
                        let pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                        weak_move.update(cx, |page, cx| page.crop_move(pos, cx)).ok();
                    })
                    .on_mouse_up(MouseButton::Left, move |_ev, _w, cx| {
                        weak_up.update(cx, |page, cx| page.crop_up(cx)).ok();
                    }),
            );

        div()
            .id("crop-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::scrim_strong())
            .child(
                div()
                    .v_flex()
                    .gap(px(14.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(20.0))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("裁剪封面（拖拽选择正方形区域）"),
                    )
                    .child(stage)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(spacing::SM))
                            .child(Button::new("crop-cancel").label("取消").on_click(move |_ev, _w, cx| {
                                weak_cancel.update(cx, |page, cx| page.crop_cancel(cx)).ok();
                            }))
                            .child(
                                Button::new("crop-ok")
                                    .primary()
                                    .label("确定")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_ok.update(cx, |page, cx| page.crop_confirm(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 重命名成功后更新页内标题。
    pub(crate) fn rename_done(&mut self, name: String, cx: &mut Context<Self>) {
        self.name = name;
        cx.notify();
    }

    /// 是否本人创建（可编辑）。
    fn owned(&self) -> bool {
        match (&self.detail, self.my_uid) {
            (Some(d), Some(uid)) => d.creator_id != 0 && d.creator_id == uid,
            _ => false,
        }
    }

    /// 移除歌单（非本人创建时=取消收藏）。
    fn remove_playlist(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let id = self.id;
        // 乐观更新：先从「我的歌单」移除。
        self.subscribed = false;
        cx.notify();
        cx.emit(PageEvent::PlaylistChanged);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_subscribe(&client, id, false)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            if !ok {
                // 云端失败则回滚乐观更新并提示。
                this.update(cx, |this, cx| {
                    this.subscribed = true;
                    this.services.toast_error("移出歌单失败，请重试", cx);
                    cx.emit(PageEvent::PlaylistChanged);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn open_add(&mut self, cx: &mut Context<Self>) {
        self.add_open = true;
        self.add_results.clear();
        cx.notify();
    }
    fn close_add(&mut self, cx: &mut Context<Self>) {
        self.add_open = false;
        cx.notify();
    }
    fn run_add_search(&mut self, cx: &mut Context<Self>) {
        let keyword = self.add_input.read(cx).value().trim().to_string();
        if keyword.is_empty() {
            return;
        }
        self.add_gen = self.add_gen.wrapping_add(1);
        let gen = self.add_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let songs = siren_core::network::api::cloud_search(&client, &keyword, 1, 20)
                .await
                .ok()
                .map(|v| siren_core::model::parse_search_songs(&v))
                .unwrap_or_default();
            let _ = tx.send(songs);
        });
        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                // 代次不符：已有更新的搜索，丢弃这次（可能乱序到达的）旧结果。
                if this.add_gen != gen {
                    return;
                }
                let urls: Vec<String> = songs.iter().map(|s| s.cover_url.clone()).collect();
                this.add_results = songs;
                this.services.covers.prefetch(urls, 40.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn add_song(&mut self, song_id: i64, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let pid = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_add_tracks(&client, pid, &[song_id])
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.add_results.retain(|s| s.id != song_id);
                } else {
                    this.services.toast_error("添加歌曲失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 添加歌曲浮层。
    fn add_overlay(&self, weak: WeakEntity<Self>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let weak_close = weak.clone();
        let mut list = div().v_flex().w_full().gap(px(spacing::XXS));
        for song in &self.add_results {
            let weak = weak.clone();
            let sid = song.id;
            let added = false;
            list = list.child(
                div()
                    .id(format!("add-{sid}"))
                    .w_full()
                    .h(px(dim::ROW_H))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(theme::RADIUS_SM))
                    .child(self.services.covers.image(&song.cover_url, dim::COVER_SONG))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .truncate()
                                    .child(song.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .truncate()
                                    .child(song.artists.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id(("add-btn", sid as usize))
                            .px(px(spacing::MD))
                            .py(px(spacing::XS))
                            .rounded_full()
                            .cursor_pointer()
                            .bg(if added { theme::surface() } else { theme::accent() })
                            .child(div().text_xs().text_color(theme::text_primary()).child("添加"))
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |page, cx| page.add_song(sid, cx)).ok();
                            }),
                    ),
            );
        }
        div()
            .id("add-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::scrim())
            .child(
                div()
                    .v_flex()
                    .w(px(420.0))
                    .h(px(520.0))
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(18.0))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("添加歌曲"),
                    )
                    .child(Input::new(&self.add_input))
                    .child(smooth_scroll("add-scroll", list, window, cx))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .child(Button::new("add-close").label("完成").on_click(move |_ev, _w, cx| {
                                weak_close.update(cx, |page, cx| page.close_add(cx)).ok();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 当前展示（过滤 + 排序后）的曲目。
    fn shown(&self) -> Vec<Song> {
        let Some(detail) = &self.detail else {
            return Vec::new();
        };
        let q = self.query.trim().to_lowercase();
        let mut tracks: Vec<Song> = detail
            .tracks
            .iter()
            .filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q) || s.artists.to_lowercase().contains(&q))
            .cloned()
            .collect();
        match self.sort {
            SortOrder::Custom => {}
            SortOrder::Name => tracks.sort_by(|a, b| a.name.cmp(&b.name)),
            SortOrder::Duration => tracks.sort_by_key(|s| s.duration_ms),
        }
        tracks
    }
}

impl Render for PlaylistPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_back = cx.weak_entity();
        let weak_star = cx.weak_entity();
        let weak_menu = cx.weak_entity();
        let owned = self.owned();
        let weak_play = cx.weak_entity();
        let weak_shuffle = cx.weak_entity();
        let weak_sort = cx.weak_entity();
        let subscribed = self.subscribed;
        let sort = self.sort;
        let pid = self.id;

        let header = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .id("nav-back")
                    .size(px(dim::ROUND_BTN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |_page, cx| cx.emit(PageEvent::Back)).ok();
                    })
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .text_color(theme::text_primary())
                    .child(self.name.clone()),
            )
            .child({
                let weak = weak_menu;
                // 删除确认浮层需要歌单名（标题 `删除"{name}"？`）。
                let pname = self.name.clone();
                Button::new("pl-more")
                    .ghost()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu(move |menu, _w, _cx| {
                        if owned {
                            let weak_edit = weak.clone();
                            let weak_cover = weak.clone();
                            let weak_rename = weak.clone();
                            let weak_del = weak.clone();
                            let pname = pname.clone();
                            menu.item(PopupMenuItem::new("编辑简介").on_click(move |_ev, _w, cx| {
                                weak_edit
                                    .update(cx, |_p, cx| cx.emit(PageEvent::EditPlaylistDesc { id: pid }))
                                    .ok();
                            }))
                            .item(PopupMenuItem::new("更换封面").on_click(move |_ev, _w, cx| {
                                weak_cover.update(cx, |page, cx| page.change_cover(cx)).ok();
                            }))
                            .item(PopupMenuItem::new("重命名").on_click(move |_ev, _w, cx| {
                                weak_rename
                                    .update(cx, |_p, cx| cx.emit(PageEvent::RenamePlaylist { id: pid }))
                                    .ok();
                            }))
                            .item(PopupMenuItem::new("删除歌单").on_click(move |_ev, _w, cx| {
                                weak_del
                                    .update(cx, |_p, cx| {
                                        cx.emit(PageEvent::DeletePlaylist {
                                            id: pid,
                                            name: pname.clone(),
                                        })
                                    })
                                    .ok();
                            }))
                        } else {
                            // 非本人创建：只能移除（取消收藏），不能编辑
                            let weak_rm = weak.clone();
                            menu.item(PopupMenuItem::new("移除歌单").on_click(move |_ev, _w, cx| {
                                weak_rm.update(cx, |page, cx| page.ask_remove_playlist(cx)).ok();
                            }))
                        }
                    })
            });

        let body: AnyElement = match &self.detail {
            Some(detail) => {
                let shown = self.shown();
                let count = detail.tracks.len();
                let total_ms: i64 = detail.tracks.iter().map(|s| s.duration_ms).sum();
                let summary = format!("{} 首歌曲，{}", count, fmt_duration(total_ms));
                let type_label = if detail.privacy == 10 {
                    "隐私歌单"
                } else {
                    "公开歌单"
                };

                let cover = self.services.covers.image(&detail.cover_url, 200.0);
                let creator_avatar = if detail.creator_avatar.is_empty() {
                    div().into_any_element()
                } else {
                    div()
                        .mr(px(spacing::SM))
                        .child(self.services.covers.image_round(&detail.creator_avatar, 24.0))
                        .into_any_element()
                };

                // 顶部 hero 背景取封面主色（上深下淡渐变）；主色未就绪时无背景。
                let base_color = self.services.covers.color(&detail.cover_url);
                let hero_bg = base_color.map(theme::cover_backdrop);

                // 随曲目区滚动收起 hero（对齐 Melodia 的 `PlaylistStickyBar`）：
                // 滚过阈值后大封面让位给「封面取色底 + 歌单名」的紧凑横栏，把纵向空间还给曲目表；
                // 滚回顶部平滑展开。`transition` 可逆，且尊重系统「减少动态效果」。
                let collapsed = smooth_scroll_offset("playlist-tracks", cx) > COLLAPSE_AT_PX;
                let fold = transition(
                    "playlist-header-collapse",
                    if collapsed { 1.0f32 } else { 0.0 },
                    Transition::new(motion::HEADER_COLLAPSE).ease(motion::ease_in_out()),
                    window,
                    cx,
                );
                // 横栏里的播放键（对齐 Melodia `StickyPlayButtonSize = 48.dp`）：
                // 操作栏收起后，这里是唯一的播放入口。
                let weak_bar_play = cx.weak_entity();
                let sticky_bar = div()
                    .w_full()
                    .h(px(STICKY_BAR_H * fold))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .px(px(spacing::XL))
                    // 底色同 Melodia：用同一封面主色的实色（不是 hero 的渐变，64px 内渐变会近乎全黑）。
                    .when_some(base_color.map(|c| theme::darken(c, 0.42)), |el, bg| el.bg(bg))
                    .child(
                        div()
                            .id("pl-bar-play")
                            .flex_none()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme::accent())
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak_bar_play
                                    .update(cx, |page, cx| {
                                        let items = page.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        let source = page.source();
                                        page.playback
                                            .update(cx, |pb, cx| pb.play_from_source(items, 0, Some(source), cx));
                                    })
                                    .ok();
                            })
                            .child(icon(IconName::Play, 22.0, theme::text_primary())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(detail.name.clone()),
                    );

                // 吸顶列头：与曲目行共用列宽（`dim::*`），滚不滚都钉在列表之上
                //（Melodia 用 stickyHeader + translationY 让表头停在横栏下，Siren 的 hero/操作栏
                // 是固定区，表头放固定区即可得到同样的吸顶效果）。
                let column_header = div()
                    .w_full()
                    .v_flex()
                    .bg(theme::pane())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .px(px(spacing::XL))
                            .py(px(spacing::SM))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(div().w(px(dim::INDEX_W)).flex().justify_end().child("#"))
                            .child(div().w(px(dim::COVER_SONG)))
                            .child(div().flex_1().min_w(px(0.0)).child("标题"))
                            .child(div().w(px(dim::ALBUM_W)).child("专辑"))
                            .child(div().w(px(dim::LIKE_W)))
                            .child(div().w(px(dim::DURATION_W)).flex().justify_end().child(icon(
                                IconName::Clock,
                                14.0,
                                theme::text_gray(),
                            ))),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(spacing::XL))
                            .child(div().w_full().h(px(1.0)).bg(theme::surface_light().opacity(0.6))),
                    );

                let hero = div()
                    .w_full()
                    .h(px(HERO_H * (1.0 - fold)))
                    .overflow_hidden()
                    .opacity(1.0 - fold)
                    .flex()
                    .items_end()
                    .gap(px(20.0))
                    .p(px(spacing::XL))
                    .when_some(hero_bg, |el, bg| el.bg(bg))
                    .child(cover)
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap(px(spacing::XXS))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child(type_label),
                            )
                            .child(
                                div()
                                    .text_color(theme::text_primary())
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(theme::text::HERO))
                                    .truncate()
                                    .child(detail.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(creator_avatar)
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme::text_primary())
                                            .child(detail.creator_nickname.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_gray())
                                            .child(format!(" · {summary}")),
                                    ),
                            ),
                    );

                let weak_add = cx.weak_entity();
                let weak_edit = cx.weak_entity();
                let action_row = div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(18.0))
                    .px(px(spacing::XL))
                    .pb(px(10.0))
                    .child(
                        div()
                            .id("pl-play")
                            .size(px(56.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme::accent())
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak_play
                                    .update(cx, |page, cx| {
                                        let items = page.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        let source = page.source();
                                        page.playback
                                            .update(cx, |pb, cx| pb.play_from_source(items, 0, Some(source), cx));
                                    })
                                    .ok();
                            })
                            .child(icon(IconName::Play, 26.0, theme::text_primary())),
                    )
                    .child(
                        div()
                            .id("pl-shuffle")
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak_shuffle
                                    .update(cx, |page, cx| {
                                        let items = page.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        page.playback.update(cx, |pb, cx| pb.play_shuffled(items, cx));
                                    })
                                    .ok();
                            })
                            .child(icon(IconName::Shuffle, 28.0, theme::text_gray())),
                    )
                    .when(!owned, |row| {
                        // 自建歌单没有「收藏」语义（点了等于收藏自己的歌单），不展示。
                        row.child(
                            div()
                                .id("pl-fav")
                                .cursor_pointer()
                                .on_click(move |_ev, _window, cx| {
                                    weak_star.update(cx, |page, cx| page.toggle_subscribe(cx)).ok();
                                })
                                .child(icon(
                                    if subscribed { IconName::Star } else { IconName::StarOff },
                                    28.0,
                                    if subscribed {
                                        theme::accent()
                                    } else {
                                        theme::text_gray()
                                    },
                                )),
                        )
                    })
                    .child(div().id("pl-download").cursor_pointer().child(icon(
                        IconName::Download,
                        28.0,
                        theme::text_gray(),
                    )))
                    .child(div().flex_1())
                    .child(
                        div()
                            .w(px(200.0))
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
                            .px(px(spacing::MD))
                            .rounded_full()
                            .bg(theme::surface())
                            .child(icon(IconName::Search, 16.0, theme::text_gray()))
                            .child(Input::new(&self.query_input)),
                    )
                    .child(
                        div()
                            .id("pl-sort")
                            .flex()
                            .items_center()
                            .gap(px(spacing::XXS))
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child(sort.label())
                            .child(icon(IconName::List, 16.0, theme::text_gray()))
                            .on_click(move |_ev, _window, cx| {
                                weak_sort
                                    .update(cx, |page, cx| {
                                        page.sort = page.sort.next();
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    );

                let pills: AnyElement = if owned {
                    let weak_add = weak_add;
                    let weak_edit = weak_edit;
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(spacing::SM))
                        .px(px(spacing::XL))
                        .pb(px(14.0))
                        .child(pill(IconName::Plus, "添加", move |_ev, _w, cx| {
                            weak_add.update(cx, |page, cx| page.open_add(cx)).ok();
                        }))
                        .child(pill(IconName::Pencil, "名称和详情", move |_ev, _w, cx| {
                            weak_edit
                                .update(cx, |_p, cx| cx.emit(PageEvent::RenamePlaylist { id: pid }))
                                .ok();
                        }))
                        .into_any_element()
                } else {
                    div().into_any_element()
                };

                let weak = cx.weak_entity();
                let covers = self.services.covers.clone();
                let playback = self.playback.clone();
                // 当前播放行高亮。
                let current = self.playback.read(cx).current_song_id();
                let filter_empty = !self.query.trim().is_empty() && shown.is_empty();
                let rows: AnyElement = if filter_empty {
                    div()
                        .w_full()
                        .py(px(spacing::XL))
                        .flex()
                        .justify_center()
                        .child(div().text_sm().text_color(theme::text_gray()).child("没有匹配的歌曲"))
                        .into_any_element()
                } else {
                    smooth_virtual_list(
                        "playlist-tracks",
                        shown.len(),
                        dim::ROW_H,
                        move |range, _window, cx| {
                            range
                                .map(|i| {
                                    let song = &shown[i];
                                    let song_owned = song.clone();
                                    let liked = playback.read(cx).is_liked(song.id);
                                    let playing = current == Some(song.id);
                                    let name_color = if playing {
                                        theme::accent()
                                    } else {
                                        theme::text_primary()
                                    };
                                    let cover = covers.image(&song.cover_url, dim::COVER_SONG);
                                    let weak = weak.clone();
                                    let weak_ctx = weak.clone();
                                    let sid = song.id;
                                    div()
                                        .id(format!("playlist-track-{}", song.id))
                                        .group(format!("playlist-track-{}", song.id))
                                        .w_full()
                                        .h(px(dim::ROW_H))
                                        .px(px(spacing::XL))
                                        .flex()
                                        .items_center()
                                        .gap(px(spacing::MD))
                                        .rounded(px(theme::RADIUS_SM))
                                        .hover(|s| s.bg(theme::card_surface()))
                                        .cursor_pointer()
                                        .on_click(move |_ev, _window, cx| {
                                            weak.update(cx, |this, cx| {
                                                let items =
                                                    this.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                                let idx = items.iter().position(|s| s.id == sid).unwrap_or(0);
                                                let source = this.source();
                                                this.playback.update(cx, |pb, cx| {
                                                    pb.play_from_source(items, idx, Some(source), cx)
                                                });
                                            })
                                            .ok();
                                        })
                                        .child(
                                            div()
                                                .w(px(dim::INDEX_W))
                                                .flex()
                                                .justify_end()
                                                .text_sm()
                                                .text_color(theme::text_gray())
                                                .child(format!("{}", i + 1)),
                                        )
                                        .child(cover)
                                        .child(
                                            div()
                                                .v_flex()
                                                .flex_1()
                                                .min_w(px(0.0))
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(name_color)
                                                        .truncate()
                                                        .child(song.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme::text_gray())
                                                        .truncate()
                                                        .child(song.artists.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .w(px(dim::ALBUM_W))
                                                .text_xs()
                                                .text_color(theme::text_gray())
                                                .truncate()
                                                .child(song.album.clone()),
                                        )
                                        .child(like_button(
                                            &playback,
                                            song.clone(),
                                            format!("playlist-track-{}", song.id),
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .w(px(dim::DURATION_W))
                                                .flex()
                                                .justify_end()
                                                .text_xs()
                                                .text_color(theme::text_gray())
                                                .child(fmt_time(song.duration_ms)),
                                        )
                                        .context_menu(move |menu, _w, _cx| {
                                            let weak_next = weak_ctx.clone();
                                            let weak_like = weak_ctx.clone();
                                            let weak_add = weak_ctx.clone();
                                            let weak_del = weak_ctx.clone();
                                            let song_next = song_owned.clone();
                                            let song_like = song_owned.clone();
                                            let song_del = song_owned.clone();
                                            menu.item(PopupMenuItem::new("下一首播放").on_click(move |_ev, _w, cx| {
                                                weak_next
                                                    .update(cx, |page, cx| {
                                                        page.playback
                                                            .update(cx, |pb, cx| pb.play_next(song_next.clone(), cx))
                                                    })
                                                    .ok();
                                            }))
                                            .item(PopupMenuItem::new(if liked { "取消喜欢" } else { "喜欢" }).on_click(
                                                move |_ev, _w, cx| {
                                                    weak_like
                                                        .update(cx, |page, cx| {
                                                            page.playback.update(cx, |pb, cx| {
                                                                pb.toggle_like_song(song_like.clone(), cx)
                                                            })
                                                        })
                                                        .ok();
                                                },
                                            ))
                                            .item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                                                weak_add
                                                    .update(cx, |_page, cx| {
                                                        cx.emit(PageEvent::AddToPlaylist { song_id: sid })
                                                    })
                                                    .ok();
                                            }))
                                            .item(
                                                PopupMenuItem::new("从歌单移除").on_click(move |_ev, _w, cx| {
                                                    let name = song_del.name.clone();
                                                    weak_del
                                                        .update(cx, |page, cx| page.ask_remove_track(sid, name, cx))
                                                        .ok();
                                                }),
                                            )
                                        })
                                        .into_any_element()
                                })
                                .collect::<Vec<_>>()
                        },
                        window,
                        cx,
                    )
                    .into_any_element()
                };

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .w_full()
                    .child(hero)
                    .child(sticky_bar)
                    // 操作栏与胶囊随 hero 一起收起（Melodia 里这两块本来就在列表内、会滚走）：
                    // 收起后只剩「横栏 + 列头」，纵向空间全给曲目表。滚回顶部自动复原。
                    .child(
                        div()
                            .w_full()
                            .h(px((ACTION_ROW_H + if owned { PILLS_H } else { 0.0 }) * (1.0 - fold)))
                            .overflow_hidden()
                            .opacity(1.0 - fold)
                            .child(action_row)
                            .child(pills),
                    )
                    .child(column_header)
                    .child(rows)
                    .into_any_element()
            }
            None => {
                if self.failed {
                    let weak = cx.weak_entity();
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(spacing::MD))
                        .child(div().text_sm().text_color(theme::text_gray()).child("歌单加载失败"))
                        .child(
                            Button::new("playlist-retry")
                                .label("重试")
                                .on_click(move |_ev, _w, cx| {
                                    weak.update(cx, |page, cx| page.load(cx)).ok();
                                }),
                        )
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_sm().text_color(theme::text_gray()).child("加载歌单中…"))
                        .into_any_element()
                }
            }
        };
        let shell = page_shell_with_header(header.into_any_element(), body).into_any_element();
        let overlay = self.crop.is_some().then(|| self.crop_overlay(cx.weak_entity()));
        let add_overlay = self.add_open.then(|| self.add_overlay(cx.weak_entity(), window, cx));
        let remove_track_overlay = self
            .remove_track_confirm
            .is_some()
            .then(|| self.remove_track_overlay(cx.weak_entity()));
        let remove_playlist_overlay = self
            .remove_playlist_confirm
            .then(|| self.remove_playlist_overlay(cx.weak_entity()));
        div()
            .relative()
            .size_full()
            .child(shell)
            .when_some(overlay, |this, overlay| this.child(overlay))
            .when_some(add_overlay, |this, overlay| this.child(overlay))
            .when_some(remove_track_overlay, |this, overlay| this.child(overlay))
            .when_some(remove_playlist_overlay, |this, overlay| this.child(overlay))
            .into_any_element()
    }
}

/// 毫秒 → 「X 小时 Y 分钟 / Y 分钟」。
fn fmt_duration(ms: i64) -> String {
    let total_min = (ms.max(0) / 60_000) as u64;
    if total_min >= 60 {
        format!("{} 小时 {} 分钟", total_min / 60, total_min % 60)
    } else {
        format!("{total_min} 分钟")
    }
}

/// 「添加 / 名称和详情」胶囊按钮（对齐 Melodia PlaylistPill）。
fn pill(
    name: IconName,
    label: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(format!("pill-{label}"))
        .flex()
        .items_center()
        .gap(px(spacing::XXS))
        .px(px(14.0))
        .h(px(spacing::XXL))
        .rounded_full()
        .bg(theme::surface())
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface_light()))
        .child(icon(name, 18.0, theme::text_primary()))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(label.to_string()),
        )
        .on_click(on_click)
}

/// 危险操作按钮（红底，用于移除/删除确认）。
fn danger_button(
    id: &'static str,
    label: &str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px(px(spacing::LG))
        .py(px(spacing::SM))
        .rounded_full()
        .cursor_pointer()
        .bg(theme::accent())
        .hover(|s| s.opacity(0.85))
        .child(
            div()
                .text_sm()
                .text_color(theme::text_primary())
                .child(label.to_string()),
        )
        .on_click(on_click)
}

/// 并发分批拉取：向 `JoinSet` 投递一首歌详情任务，返回 `(本批数量, 用时, 结果)`。
fn spawn_song_batch(
    set: &mut tokio::task::JoinSet<(usize, std::time::Duration, siren_core::error::Result<Vec<Song>>)>,
    client: &siren_core::network::client::NeteaseClient,
    chunk: Vec<i64>,
) {
    let client = client.clone();
    let n = chunk.len();
    set.spawn(async move {
        let started = std::time::Instant::now();
        let result = siren_core::network::api::song_details(&client, &chunk).await;
        (n, started.elapsed(), result)
    });
}
