//! 最近播放：需登录，展示最近播放的歌曲；顶部为本地记录的「最近播放歌单」。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::Song;
use siren_core::recent_playlists::LocalRecentPlaylist;

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct RecentPage {
    services: Services,
    playback: Entity<Playback>,
    songs: Arc<Vec<Song>>,
    /// 本地记录的最近播放歌单（新的在前）。
    playlists: Vec<LocalRecentPlaylist>,
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
            playlists: Vec::new(),
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
                // 本地记录的最近播放歌单（同步读取）
                this.playlists = siren_core::recent_playlists::load_recent_playlists();
                match songs {
                    Some(items) => {
                        let urls: Vec<String> = items.iter().map(|s| s.cover_url.clone()).collect();
                        this.status = format!("最近播放 · {} 首", items.len()).into();
                        this.songs = Arc::new(items);
                        this.services.covers.prefetch(urls, 40.0, cx);
                    }
                    None => this.status = "最近播放（需登录）".into(),
                }
                let playlist_covers: Vec<String> = this.playlists.iter().map(|p| p.cover_url.clone()).collect();
                this.services.covers.prefetch(playlist_covers, 108.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 最近播放歌单（横向卡片）。
    fn playlists_row(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut row = div().id("recent-playlists").flex().gap(px(12.0)).pb(px(4.0));
        for item in &self.playlists {
            let cover = covers.image(&item.cover_url, 108.0);
            let weak = weak.clone();
            let (id, name) = (item.id, item.name.clone());
            row = row.child(
                div()
                    .id(format!("recent-pl-{id}"))
                    .w(px(108.0))
                    .flex_none()
                    .v_flex()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_page, cx| {
                            cx.emit(PageEvent::OpenPlaylist { id, name: name.clone() });
                        })
                        .ok();
                    })
                    .child(cover)
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(item.name.clone()),
                    ),
            );
        }
        row.into_any_element()
    }

    fn songs_list(&self, cx: &Context<Self>) -> AnyElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        uniform_list("recent-songs", count, move |range, _window, _cx| {
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
        .min_w(px(0.0))
        .into_any_element()
    }
}

impl Render for RecentPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(self.status.to_string())
            .into_any_element();

        let mut body = div().v_flex().flex_1().min_w(px(0.0)).p(px(16.0)).gap(px(12.0));
        if !self.playlists.is_empty() {
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("最近播放歌单"),
                    )
                    .child(self.playlists_row(cx)),
            );
        }
        body = body.child(
            div()
                .v_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("最近播放歌曲"),
                )
                .child(self.songs_list(cx)),
        );

        page_shell_with_header(header, body.into_any_element())
    }
}
