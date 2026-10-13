//! 最近播放：顶部为「最近播放歌单 / 专辑」横向卡片，下方为按天分组的「最近播放歌曲」。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::{RecentAlbum, RecentPlaylist, RecentSong};

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{fmt_day_label, fmt_time, page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct RecentPage {
    services: Services,
    playback: Entity<Playback>,
    songs: Vec<RecentSong>,
    playlists: Vec<RecentPlaylist>,
    albums: Vec<RecentAlbum>,
    status: SharedString,
    load_gen: u64,
}

impl EventEmitter<PageEvent> for RecentPage {}

impl RecentPage {
    pub(crate) fn new(services: Services, playback: Entity<Playback>) -> Self {
        Self {
            services,
            playback,
            songs: Vec::new(),
            playlists: Vec::new(),
            albums: Vec::new(),
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
            let songs = siren_core::network::api::recent_songs_with_time(&client)
                .await
                .unwrap_or_default();
            let playlists = siren_core::network::api::recent_playlists(&client)
                .await
                .unwrap_or_default();
            let albums = siren_core::network::api::recent_albums(&client)
                .await
                .unwrap_or_default();
            let _ = tx.send((songs, playlists, albums));
        });
        self.status = "加载最近播放…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((songs, playlists, albums)) = payload {
                    // 本地最近播放歌单与服务端合并去重（按播放时间倒序）
                    let mut merged: Vec<RecentPlaylist> = siren_core::recent_playlists::load_recent_playlists()
                        .into_iter()
                        .map(|p| RecentPlaylist {
                            id: p.id,
                            name: p.name,
                            cover_url: p.cover_url,
                            creator_name: String::new(),
                            play_time: p.play_time,
                        })
                        .collect();
                    merged.extend(playlists);
                    merged.sort_by_key(|p| std::cmp::Reverse(p.play_time));
                    let mut seen = std::collections::HashSet::new();
                    merged.retain(|p| seen.insert(p.id));
                    merged.truncate(100);

                    let covers: Vec<String> = songs.iter().map(|s| s.song.cover_url.clone()).collect();
                    this.services.covers.prefetch(covers, 40.0, cx);
                    this.services
                        .covers
                        .prefetch(merged.iter().map(|p| p.cover_url.clone()), 108.0, cx);
                    this.services
                        .covers
                        .prefetch(albums.iter().map(|a| a.cover_url.clone()), 108.0, cx);

                    this.status = format!("最近播放 · {} 首", songs.len()).into();
                    this.songs = songs;
                    this.playlists = merged;
                    this.albums = albums;
                } else {
                    this.status = "最近播放（需登录）".into();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn card_row(
        &self,
        id_prefix: &'static str,
        items: Vec<(i64, String, String)>,
        on_open: fn(i64, String) -> PageEvent,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut row = div().id(id_prefix).flex().gap(px(12.0));
        for (id, name, cover_url) in items {
            let cover = covers.image(&cover_url, 108.0);
            let weak = weak.clone();
            let label = name.clone();
            row = row.child(
                div()
                    .id(format!("{id_prefix}-{id}"))
                    .w(px(108.0))
                    .flex_none()
                    .v_flex()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_page, cx| cx.emit(on_open(id, name.clone()))).ok();
                    })
                    .child(cover)
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(label),
                    ),
            );
        }
        row.into_any_element()
    }

    fn songs_grouped(&self, now_ms: i64, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let songs: Arc<Vec<RecentSong>> = Arc::new(self.songs.clone());
        let mut col = div().v_flex().w_full();
        let mut current_label = String::from("\u{0}");
        for (i, item) in songs.iter().enumerate() {
            let label = fmt_day_label(item.play_time, now_ms);
            if label != current_label {
                current_label = label.clone();
                if !label.is_empty() {
                    col = col.child(
                        div()
                            .w_full()
                            .pt(px(10.0))
                            .pb(px(2.0))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(label),
                    );
                }
            }
            let cover = covers.image(&item.song.cover_url, 40.0);
            let weak = weak.clone();
            let weak_ctx = weak.clone();
            let idx = i;
            let sid = item.song.id;
            col = col.child(
                div()
                    .id(format!("recent-song-{sid}"))
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
                            let items: Vec<siren_core::model::Song> =
                                this.songs.iter().map(|s| s.song.clone()).collect();
                            this.playback.update(cx, |pb, cx| pb.play_from(items, idx, cx));
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
                                    .child(item.song.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .truncate()
                                    .child(item.song.artists.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(56.0))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(fmt_time(item.song.duration_ms)),
                    )
                    .context_menu(move |menu, _w, _cx| {
                        let weak_ctx = weak_ctx.clone();
                        menu.item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                            weak_ctx
                                .update(cx, |_page, cx| cx.emit(PageEvent::AddToPlaylist { song_id: sid }))
                                .ok();
                        }))
                    }),
            );
        }
        col.into_any_element()
    }
}

impl Render for RecentPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let header = div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(self.status.to_string())
            .into_any_element();

        let mut body = div()
            .id("recent-scroll")
            .v_flex()
            .flex_1()
            .min_w(px(0.0))
            .p(px(16.0))
            .gap(px(12.0))
            .overflow_y_scroll();

        let playlist_items: Vec<(i64, String, String)> = self
            .playlists
            .iter()
            .map(|p| (p.id, p.name.clone(), p.cover_url.clone()))
            .collect();
        if !playlist_items.is_empty() {
            body = body.child(section(
                "最近播放歌单",
                self.card_row(
                    "recent-pl",
                    playlist_items,
                    |id, name| PageEvent::OpenPlaylist { id, name },
                    cx,
                ),
            ));
        }

        let album_items: Vec<(i64, String, String)> = self
            .albums
            .iter()
            .map(|a| (a.id, a.name.clone(), a.cover_url.clone()))
            .collect();
        if !album_items.is_empty() {
            body = body.child(section(
                "最近播放专辑",
                self.card_row(
                    "recent-al",
                    album_items,
                    |id, name| PageEvent::OpenAlbum { id, name },
                    cx,
                ),
            ));
        }

        body = body.child(section("最近播放歌曲", self.songs_grouped(now_ms, cx)));

        page_shell_with_header(header, body.into_any_element())
    }
}

fn section(title: &str, content: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(8.0))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(content)
        .into_any_element()
}
