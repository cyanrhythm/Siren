//! 搜索：多类型结果（单曲 / 专辑 / 歌手 / 歌单）。输入框在标题栏。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Album, Artist, Playlist, Song};

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{page_shell, PageEvent};
use crate::theme;

/// 搜索类型（`api_value` 对齐 Melodia `SearchType`）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchKind {
    Song,
    Album,
    Artist,
    Playlist,
}

impl SearchKind {
    const ALL: [SearchKind; 4] = [
        SearchKind::Song,
        SearchKind::Album,
        SearchKind::Artist,
        SearchKind::Playlist,
    ];

    fn api_value(self) -> u32 {
        match self {
            SearchKind::Song => 1,
            SearchKind::Album => 10,
            SearchKind::Artist => 100,
            SearchKind::Playlist => 1000,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SearchKind::Song => "单曲",
            SearchKind::Album => "专辑",
            SearchKind::Artist => "歌手",
            SearchKind::Playlist => "歌单",
        }
    }
}

pub(crate) struct SearchPage {
    services: Services,
    playback: Entity<Playback>,
    keyword: String,
    kind: SearchKind,
    songs: Arc<Vec<Song>>,
    albums: Arc<Vec<Album>>,
    artists: Arc<Vec<Artist>>,
    playlists: Arc<Vec<Playlist>>,
    selected: Option<usize>,
    status: SharedString,
    run_gen: u64,
}

impl EventEmitter<PageEvent> for SearchPage {}

impl SearchPage {
    pub(crate) fn new(services: Services, playback: Entity<Playback>) -> Self {
        Self {
            services,
            playback,
            keyword: String::new(),
            kind: SearchKind::Song,
            songs: Arc::new(Vec::new()),
            albums: Arc::new(Vec::new()),
            artists: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            selected: None,
            status: "在顶部搜索框输入关键词并回车".into(),
            run_gen: 0,
        }
    }

    /// 由标题栏回车触发：设置关键词并按当前类型检索。
    pub(crate) fn search(&mut self, keyword: String, cx: &mut Context<Self>) {
        self.keyword = keyword;
        self.run(cx);
    }

    /// 切换结果类型并重搜。
    fn set_kind(&mut self, kind: SearchKind, cx: &mut Context<Self>) {
        if self.kind == kind {
            return;
        }
        self.kind = kind;
        if self.keyword.is_empty() {
            cx.notify();
            return;
        }
        self.run(cx);
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        if self.keyword.is_empty() {
            return;
        }
        self.run_gen = self.run_gen.wrapping_add(1);
        let gen = self.run_gen;
        let kind = self.kind;
        let keyword = self.keyword.clone();
        self.status = format!("搜索中：{keyword}").into();
        cx.notify();

        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let value = siren_core::network::api::cloud_search(&client, &keyword, kind.api_value(), 30)
                .await
                .ok();
            let _ = tx.send(value);
        });

        cx.spawn(async move |this, cx| {
            let value = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if this.run_gen != gen {
                    return;
                }
                let Some(value) = value else {
                    this.status = format!("{} · 0 条", kind.label()).into();
                    cx.notify();
                    return;
                };
                let count = match kind {
                    SearchKind::Song => {
                        let items = siren_core::model::parse_search_songs(&value);
                        let urls: Vec<String> = items.iter().map(|s| s.cover_url.clone()).collect();
                        this.services.covers.prefetch(urls, 40.0, cx);
                        this.songs = Arc::new(items);
                        this.songs.len()
                    }
                    SearchKind::Album => {
                        let items = siren_core::model::parse_search_albums(&value);
                        let urls: Vec<String> = items.iter().map(|a| a.cover_url.clone()).collect();
                        this.services.covers.prefetch(urls, 48.0, cx);
                        this.albums = Arc::new(items);
                        this.albums.len()
                    }
                    SearchKind::Artist => {
                        let items = siren_core::model::parse_search_artists(&value);
                        let urls: Vec<String> = items.iter().map(|a| a.cover_url.clone()).collect();
                        this.services.covers.prefetch(urls, 48.0, cx);
                        this.artists = Arc::new(items);
                        this.artists.len()
                    }
                    SearchKind::Playlist => {
                        let items = siren_core::model::parse_search_playlists(&value);
                        let urls: Vec<String> = items.iter().map(|p| p.cover_url.clone()).collect();
                        this.services.covers.prefetch(urls, 48.0, cx);
                        this.playlists = Arc::new(items);
                        this.playlists.len()
                    }
                };
                this.status = format!("{} · {} 条", kind.label(), count).into();
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
        let weak = cx.weak_entity();
        let tabs = {
            let mut row = div().flex().items_center().gap(px(8.0)).px(px(16.0)).pt(px(4.0));
            for kind in SearchKind::ALL {
                let active = kind == self.kind;
                let weak = weak.clone();
                row = row.child(
                    div()
                        .id(("search-kind", kind as usize))
                        .px(px(12.0))
                        .py(px(5.0))
                        .rounded_full()
                        .cursor_pointer()
                        .bg(if active { theme::accent() } else { theme::surface() })
                        .when(!active, |s| s.hover(|s| s.bg(theme::card_surface())))
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |this, cx| this.set_kind(kind, cx)).ok();
                        })
                        .child(div().text_sm().text_color(theme::text_primary()).child(kind.label())),
                );
            }
            row
        };

        let body: AnyElement = match self.kind {
            SearchKind::Song => self.song_list(cx),
            SearchKind::Album => self.album_list(cx),
            SearchKind::Artist => self.artist_list(cx),
            SearchKind::Playlist => self.playlist_list(cx),
        };

        let content = div().v_flex().flex_1().min_w(px(0.0)).w_full().child(tabs).child(body);

        page_shell(format!("搜索 · {}", self.status), content.into_any_element())
    }
}

impl SearchPage {
    fn song_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        uniform_list("search-songs", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let song = &songs[i];
                    let marker = if selected == Some(i) { "▶ " } else { "" };
                    let cover = covers.image(&song.cover_url, 40.0);
                    let weak = weak.clone();
                    let weak_ctx = weak.clone();
                    let sid = song.id;
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

    fn album_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let items = self.albums.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let render = uniform_list("search-albums", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let album = &items[i];
                    let cover = covers.image(&album.cover_url, 48.0);
                    let weak = weak.clone();
                    let id = album.id;
                    let name = album.name.clone();
                    div()
                        .id(format!("search-album-{}", album.id))
                        .w_full()
                        .h(px(60.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(theme::card_surface()))
                        .cursor_pointer()
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |_page, cx| {
                                cx.emit(PageEvent::OpenAlbum { id, name: name.clone() });
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
                                        .child(album.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .truncate()
                                        .child(album.artists.clone()),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .w_full()
        .min_w(px(0.0));
        render.into_any_element()
    }

    fn artist_list(&self, _cx: &mut Context<Self>) -> AnyElement {
        let items = self.artists.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let render = uniform_list("search-artists", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let artist = &items[i];
                    let cover = covers.image_round(&artist.cover_url, 44.0);
                    div()
                        .id(format!("search-artist-{}", artist.id))
                        .w_full()
                        .h(px(56.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(theme::card_surface()))
                        .child(cover)
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .truncate()
                                .child(artist.name.clone()),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .w_full()
        .min_w(px(0.0));
        render.into_any_element()
    }

    fn playlist_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let items = self.playlists.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let render = uniform_list("search-playlists", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let pl = &items[i];
                    let cover = covers.image(&pl.cover_url, 48.0);
                    let weak = weak.clone();
                    let id = pl.id;
                    let name = pl.name.clone();
                    div()
                        .id(format!("search-playlist-{id}"))
                        .w_full()
                        .h(px(60.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(theme::card_surface()))
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
                                .v_flex()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_primary())
                                        .truncate()
                                        .child(pl.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .child(format!("{} 首", pl.track_count)),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        })
        .flex_1()
        .w_full()
        .min_w(px(0.0));
        render.into_any_element()
    }
}
