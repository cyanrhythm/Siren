//! 专辑详情：返回 + 收藏 + 曲目列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::AlbumDetail;

use super::super::components::{song_row, IndexCell, SongRow};
use super::super::playback::Playback;
use super::super::services::Services;
use super::super::smooth_scroll::smooth_virtual_list;
use super::super::{collapse_box, column_header, header_fold, icon, wheel_to_list, Column, PageEvent};
use super::dim;
use crate::theme::{self, spacing};

/// 顶部 hero 区块高度：`p(LG) * 2 + 封面 120`。
const HERO_BLOCK_H: f32 = 152.0;
/// 播放键操作栏高度：播放键 56 + `pb(SM)`。
const ACTION_ROW_H: f32 = 64.0;
use siren_core::player::{PlaySource, PlaySourceKind};

pub(crate) struct AlbumPage {
    services: Services,
    playback: Entity<Playback>,
    id: i64,
    name: String,
    detail: Option<Arc<AlbumDetail>>,
    /// 加载失败：用于展示错误态与重试（而非永久停在“加载中”）。
    failed: bool,
    subscribed: bool,
    load_gen: u64,
}

impl EventEmitter<PageEvent> for AlbumPage {}

impl AlbumPage {
    pub(crate) fn new(
        cx: &mut Context<Self>,
        services: Services,
        playback: Entity<Playback>,
        id: i64,
        name: String,
    ) -> Self {
        let mut page = Self {
            services,
            playback,
            id,
            name,
            detail: None,
            failed: false,
            subscribed: false,
            load_gen: 0,
        };
        page.load(cx);
        page
    }

    /// 队列来源容器。
    fn source(&self) -> PlaySource {
        PlaySource {
            kind: PlaySourceKind::Album,
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
            let detail = siren_core::network::api::album_detail(&client, id).await.ok().flatten();
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
                    let urls: Vec<String> = d.songs.iter().map(|s| s.cover_url.clone()).collect();
                    this.services.covers.prefetch(urls, 40.0, cx);
                    this.services.covers.prefetch([d.cover_url.clone()], 120.0, cx);
                }
                this.failed = detail.is_none();
                // 回填真实收藏态，否则已收藏专辑显示为未收藏、无法取消（K2）。
                this.subscribed = detail.as_ref().map(|d| d.subscribed).unwrap_or(false);
                this.detail = detail.map(Arc::new);
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
            let ok = siren_core::network::api::album_subscribe(&client, id, subscribe)
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
                            "已收藏专辑"
                        } else {
                            "已取消收藏"
                        },
                        cx,
                    );
                } else {
                    this.subscribed = !subscribe;
                    this.services.toast_error("收藏专辑失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for AlbumPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_star = cx.weak_entity();
        let weak_play = cx.weak_entity();
        let subscribed = self.subscribed;
        // 顶部收起因子（0 展开 / 1 收起）：hero、操作栏、横栏共用。
        let fold = header_fold(
            "album-header-collapse",
            "album-tracks",
            dim::HEADER_COLLAPSE_AT_PX,
            window,
            cx,
        );
        // 页面顶部不再有自己的导航行（返回键在应用标题栏、专辑名在 hero 里，皆重复）；
        // 原来的收藏星标下沉到操作栏（与歌单页把 ⋯ 下沉到操作栏同一处理）。
        let star_button = div()
            .id("album-star")
            .size(px(dim::ROUND_BTN))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(|s| s.bg(theme::surface()))
            .on_click(move |_ev, _window, cx| {
                weak_star.update(cx, |p, cx| p.toggle_subscribe(cx)).ok();
            })
            .child(icon(
                if subscribed { IconName::Star } else { IconName::StarOff },
                24.0,
                if subscribed {
                    theme::accent()
                } else {
                    theme::text_gray()
                },
            ))
            .into_any_element();

        let body: AnyElement = match &self.detail {
            Some(detail) => {
                let cover = self.services.covers.image(&detail.cover_url, 120.0);
                let songs = detail.songs.clone();
                let count = songs.len();
                let covers = self.services.covers.clone();
                let weak = cx.weak_entity();
                let playback = self.playback.clone();
                // 当前播放行高亮。
                let current = self.playback.read(cx).current_song_id();
                let rows: AnyElement = if count == 0 {
                    // 曲目为空的专辑（如新碟预告）给空态，而不是空列表。
                    div()
                        .w_full()
                        .py(px(spacing::XL))
                        .flex()
                        .justify_center()
                        .child(div().text_sm().text_color(theme::text_gray()).child("该专辑暂无曲目"))
                        .into_any_element()
                } else {
                    smooth_virtual_list(
                        "album-tracks",
                        count,
                        dim::ROW_H,
                        move |range, _window, cx| {
                            range
                                .map(|i| {
                                    let song = &songs[i];
                                    let weak = weak.clone();
                                    song_row(
                                        SongRow::new(
                                            "album-track",
                                            song,
                                            &playback,
                                            &covers,
                                            weak.clone(),
                                            cx,
                                            move |_ev, _window, cx| {
                                                weak.update(cx, |this, cx| {
                                                    let items = this
                                                        .detail
                                                        .as_ref()
                                                        .map(|d| d.songs.clone())
                                                        .unwrap_or_default();
                                                    let source = this.source();
                                                    this.playback.update(cx, |pb, cx| {
                                                        pb.play_from_source(items, i, Some(source), cx)
                                                    });
                                                })
                                                .ok();
                                            },
                                        )
                                        .row_h(dim::ROW_H)
                                        .pad(spacing::LG)
                                        .index(IndexCell::number(i, dim::INDEX_W, false))
                                        .current(current == Some(song.id)),
                                    )
                                })
                                .collect::<Vec<_>>()
                        },
                        window,
                        cx,
                    )
                    .into_any_element()
                };

                // 收起后的横栏（对齐 Melodia 的同款 sticky bar）：封面取色实色底 +
                // 播放键 + 专辑名（与歌单页横栏同形）；收藏星标下沉到操作栏，故这里不再有它。
                let weak_bar_play = cx.weak_entity();
                let bar_cover = self.services.covers.color(&detail.cover_url);
                let sticky_bar = div()
                    .w_full()
                    .rounded_t(px(theme::RADIUS_MD))
                    .h(px(dim::STICKY_BAR_H * fold))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .px(px(spacing::MD))
                    .when_some(bar_cover.map(|c| theme::darken(c, 0.42)), |el, bg| el.bg(bg))
                    .child(
                        div()
                            .id("album-bar-play")
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
                                        let items = page.detail.as_ref().map(|d| d.songs.clone()).unwrap_or_default();
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

                // 吸顶列头：专辑行没有「专辑」列，故列头为 # / 标题 / 时长（列宽与行共用 `dim::*`）。
                let column_header = column_header(
                    spacing::LG,
                    &[
                        Column::Index,
                        Column::Cover,
                        Column::Title,
                        Column::Like,
                        Column::Duration,
                    ],
                );

                wheel_to_list(
                    div()
                        .id("album-body")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .w_full()
                    // 顶部 hero：收起时让位给横栏（padding 放内层，见 collapse_box 注释）。
                    .child(collapse_box(
                        fold,
                        HERO_BLOCK_H,
                        div()
                            .w_full()
                            // 顶部圆角跟着页面外壳走（gpui 裁子元素时用矩形遮罩）。
                            .rounded_t(px(theme::RADIUS_MD))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .p(px(spacing::LG))
                            // 顶部背景取封面主色（上深下淡渐变）。
                            .when_some(
                                self.services.covers.color(&detail.cover_url).map(theme::cover_backdrop),
                                |el, bg| el.bg(bg),
                            )
                            .child(cover)
                            .child(
                                div()
                                    .v_flex()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(spacing::XS))
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme::text_primary())
                                            .child(detail.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_gray())
                                            .truncate()
                                            .child(detail.artists.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_gray())
                                            .child(format!("{} 首", detail.songs.len())),
                                    ),
                            )
                            .into_any_element(),
                    ))
                    .child(collapse_box(
                        fold,
                        ACTION_ROW_H,
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(spacing::LG))
                            .px(px(spacing::LG))
                            .pb(px(spacing::SM))
                            // 播放键在前、收藏在后（与歌单页动作栏同序：主动作靠左）。
                            .when(count > 0, |el| {
                                el.child(
                                    div()
                                        .id("album-play")
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
                                                    let items = page
                                                        .detail
                                                        .as_ref()
                                                        .map(|d| d.songs.clone())
                                                        .unwrap_or_default();
                                                    let source = page.source();
                                                    page.playback.update(cx, |pb, cx| {
                                                        pb.play_from_source(items, 0, Some(source), cx)
                                                    });
                                                })
                                                .ok();
                                        })
                                        .child(icon(IconName::Play, 26.0, theme::text_primary())),
                                )
                            })
                            .child(star_button)
                            .into_any_element(),
                    ))
                    .child(sticky_bar)
                    .child(column_header)
                    .child(rows),
                    "album-tracks",
                )
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
                        .child(div().text_sm().text_color(theme::text_gray()).child("专辑加载失败"))
                        .child(Button::new("album-retry").label("重试").on_click(move |_ev, _w, cx| {
                            weak.update(cx, |page, cx| page.load(cx)).ok();
                        }))
                        .into_any_element()
                } else {
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().text_sm().text_color(theme::text_gray()).child("加载专辑中…"))
                        .into_any_element()
                }
            }
        };
        // 外壳：页面顶部不再有自己的导航行，收起后顶部只剩横栏一行（同歌单页）。
        div()
            .v_flex()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .rounded(px(theme::RADIUS_MD))
            .overflow_hidden()
            .bg(theme::pane())
            .child(body)
    }
}
