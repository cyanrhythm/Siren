//! 歌单详情：返回 + 曲目列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::PlaylistDetail;

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{cover_placeholder, icon, page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct PlaylistPage {
    services: Services,
    playback: Entity<Playback>,
    id: i64,
    name: String,
    detail: Option<Arc<PlaylistDetail>>,
}

impl EventEmitter<PageEvent> for PlaylistPage {}

impl PlaylistPage {
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
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let id = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let detail = siren_core::network::api::playlist_detail(&client, id)
                .await
                .ok()
                .flatten();
            let _ = tx.send(detail);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.detail = detail.map(Arc::new);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for PlaylistPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_back = cx.weak_entity();
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .id("nav-back")
                    .size(px(30.0))
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
                    .text_lg()
                    .text_color(theme::text_primary())
                    .child(self.name.clone()),
            );

        let body: AnyElement = match &self.detail {
            Some(detail) => {
                let tracks = detail.tracks.clone();
                let count = tracks.len();
                let weak = cx.weak_entity();
                uniform_list("playlist-tracks", count, move |range, _window, _cx| {
                    range
                        .map(|i| {
                            let song = &tracks[i];
                            let weak = weak.clone();
                            div()
                                .id(format!("playlist-track-{}", song.id))
                                .w_full()
                                .h(px(48.0))
                                .px(px(16.0))
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .rounded(px(6.0))
                                .hover(|s| s.bg(theme::card_surface()))
                                .cursor_pointer()
                                .on_click(move |_ev, _window, cx| {
                                    weak.update(cx, |this, cx| {
                                        let items = this.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                                    })
                                    .ok();
                                })
                                .child(
                                    div()
                                        .w(px(24.0))
                                        .text_sm()
                                        .text_color(theme::text_gray())
                                        .child(format!("{}", i + 1)),
                                )
                                .child(cover_placeholder(36.0))
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
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                })
                .flex_1()
                .w_full()
                .min_w(px(0.0))
                .into_any_element()
            }
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(theme::text_gray()).child("加载歌单中…"))
                .into_any_element(),
        };
        page_shell_with_header(header.into_any_element(), body)
    }
}
