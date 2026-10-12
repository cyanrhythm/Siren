//! 搜索：结果列表（输入框在标题栏）。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::Song;

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{cover_placeholder, page_shell};
use crate::theme;

pub(crate) struct SearchPage {
    services: Services,
    playback: Entity<Playback>,
    songs: Arc<Vec<Song>>,
    selected: Option<usize>,
    status: SharedString,
}

impl SearchPage {
    pub(crate) fn new(services: Services, playback: Entity<Playback>) -> Self {
        Self {
            services,
            playback,
            songs: Arc::new(Vec::new()),
            selected: None,
            status: "在顶部搜索框输入关键词并回车".into(),
        }
    }

    /// 由标题栏回车触发。
    pub(crate) fn search(&mut self, keyword: String, cx: &mut Context<Self>) {
        self.status = format!("搜索中：{keyword}").into();
        cx.notify();

        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let songs = siren_core::network::api::search(&client, &keyword, 1, 50)
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });

        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.status = format!("搜索到 {} 首", songs.len()).into();
                this.songs = Arc::new(songs);
                this.selected = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for SearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let weak = cx.weak_entity();
        let list = uniform_list("search-results", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let song = &songs[i];
                    let marker = if selected == Some(i) { "▶ " } else { "   " };
                    let weak = weak.clone();
                    div()
                        .id(format!("search-song-{}", song.id))
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
                                let items = this.songs.as_ref().clone();
                                this.selected = Some(i);
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
                                        .child(format!("{marker}{}", song.name)),
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
        .min_w(px(0.0));

        page_shell(format!("搜索 · {}", self.status), list.into_any_element())
    }
}
