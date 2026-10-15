//! 专辑详情：返回 + 收藏 + 曲目列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::AlbumDetail;

use super::super::components::like_button;
use super::super::playback::Playback;
use super::super::services::Services;
use super::super::smooth_scroll::smooth_virtual_list;
use super::super::{fmt_time, icon, page_shell_with_header, PageEvent};
use crate::theme;
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
                    this.services.toast_error("操作失败", cx);
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
        let weak_back = cx.weak_entity();
        let weak_star = cx.weak_entity();
        let subscribed = self.subscribed;
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .id("album-back")
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |_p, cx| cx.emit(PageEvent::Back)).ok();
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
            .child(
                div()
                    .id("album-subscribe")
                    .size(px(30.0))
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
                        18.0,
                        if subscribed {
                            theme::accent()
                        } else {
                            theme::text_gray()
                        },
                    )),
            );

        let body: AnyElement = match &self.detail {
            Some(detail) => {
                let cover = self.services.covers.image(&detail.cover_url, 120.0);
                let songs = detail.songs.clone();
                let count = songs.len();
                let covers = self.services.covers.clone();
                let weak = cx.weak_entity();
                let playback = self.playback.clone();
                let rows = smooth_virtual_list(
                    "album-tracks",
                    count,
                    52.0,
                    move |range, _window, cx| {
                        range
                            .map(|i| {
                                let song = &songs[i];
                                let cover = covers.image(&song.cover_url, 40.0);
                                let weak = weak.clone();
                                let weak_ctx = weak.clone();
                                let song_owned = song.clone();
                                let liked = playback.read(cx).is_liked(song.id);
                                div()
                                    .id(format!("album-track-{}", song.id))
                                    .group(format!("album-track-{}", song.id))
                                    .w_full()
                                    .h(px(52.0))
                                    .px(px(16.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.0))
                                    .rounded(px(6.0))
                                    .hover(|s| s.bg(theme::card_surface()))
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak.update(cx, |this, cx| {
                                            let items =
                                                this.detail.as_ref().map(|d| d.songs.clone()).unwrap_or_default();
                                            let source = this.source();
                                            this.playback
                                                .update(cx, |pb, cx| pb.play_from_source(items, i, Some(source), cx));
                                        })
                                        .ok();
                                    })
                                    .context_menu(move |menu, _w, _cx| {
                                        let w_next = weak_ctx.clone();
                                        let w_like = weak_ctx.clone();
                                        let s_next = song_owned.clone();
                                        let s_like = song_owned.clone();
                                        menu.item(PopupMenuItem::new("下一首播放").on_click(move |_e, _w, cx| {
                                            w_next
                                                .update(cx, |page, cx| {
                                                    page.playback.update(cx, |pb, cx| pb.play_next(s_next.clone(), cx))
                                                })
                                                .ok();
                                        }))
                                        .item(
                                            PopupMenuItem::new(if liked { "取消喜欢" } else { "喜欢" }).on_click(
                                                move |_e, _w, cx| {
                                                    w_like
                                                        .update(cx, |page, cx| {
                                                            page.playback.update(cx, |pb, cx| {
                                                                pb.toggle_like_song(s_like.clone(), cx)
                                                            })
                                                        })
                                                        .ok();
                                                },
                                            ),
                                        )
                                    })
                                    .child(
                                        div()
                                            .w(px(28.0))
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
                                    .child(like_button(
                                        &playback,
                                        song.clone(),
                                        format!("album-track-{}", song.id),
                                        cx,
                                    ))
                                    .child(
                                        div()
                                            .w(px(56.0))
                                            .text_xs()
                                            .text_color(theme::text_gray())
                                            .child(fmt_time(song.duration_ms)),
                                    )
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    },
                    window,
                    cx,
                );

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .w_full()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .p(px(16.0))
                            .child(cover)
                            .child(
                                div()
                                    .v_flex()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(4.0))
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
                            ),
                    )
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
                        .gap(px(12.0))
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
        page_shell_with_header(header.into_any_element(), body)
    }
}
