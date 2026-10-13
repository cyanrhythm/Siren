//! 最近播放：需登录，展示最近播放的歌曲。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::Song;

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{page_shell, PageEvent};
use crate::theme;

pub(crate) struct RecentPage {
    services: Services,
    playback: Entity<Playback>,
    songs: Arc<Vec<Song>>,
    status: SharedString,
    load_gen: u64,
}

impl EventEmitter<PageEvent> for RecentPage {}

impl RecentPage {
    pub(crate) fn new(services: Services, playback: Entity<Playback>) -> Self {
        Self {
            services,
            playback,
            songs: Arc::new(Vec::new()),
            status: "最近播放".into(),
            load_gen: 0,
        }
    }

    /// 拉取最近播放（登录后 / 进入页面时）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let songs = siren_core::network::api::recent_songs(&client).await.ok();
            let _ = tx.send(songs);
        });
        self.status = "加载最近播放…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let songs = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                match songs {
                    Some(items) => {
                        let urls: Vec<String> = items.iter().map(|s| s.cover_url.clone()).collect();
                        this.status = format!("最近播放 · {} 首", items.len()).into();
                        this.songs = Arc::new(items);
                        this.services.covers.prefetch(urls, 40.0, cx);
                    }
                    None => this.status = "最近播放（需登录）".into(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for RecentPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let list = uniform_list("recent-songs", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let song = &songs[i];
                    let cover = covers.image(&song.cover_url, 40.0);
                    let weak = weak.clone();
                    let weak_ctx = weak.clone();
                    let sid = song.id;
                    div()
                        .id(format!("recent-song-{}", song.id))
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
                                this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                            })
                            .ok();
                        })
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
                        .child(
                            div()
                                .w(px(56.0))
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(super::super::fmt_time(song.duration_ms)),
                        )
                        .context_menu(move |menu, _w, _cx| {
                            let weak_ctx = weak_ctx.clone();
                            menu.item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                                weak_ctx
                                    .update(cx, |_page, cx| cx.emit(PageEvent::AddToPlaylist { song_id: sid }))
                                    .ok();
                            }))
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .w_full()
        .min_w(px(0.0));

        page_shell(self.status.to_string(), list.into_any_element())
    }
}
