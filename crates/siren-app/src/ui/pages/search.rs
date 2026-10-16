//! 搜索：多类型结果（单曲 / 专辑 / 歌手 / 歌单）。输入框在标题栏。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Album, Artist, Playlist, Song};

use super::super::components::{song_row, IndexCell, SongRow};
use super::super::playback::Playback;
use super::super::services::Services;
use super::super::smooth_scroll::smooth_virtual_list;
use super::super::{page_shell, PageEvent};
use super::dim;
use crate::theme::{self, spacing};

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
    /// 最近一次搜索是否失败（与「0 条」区分：失败显示错误 + 重试，K4）。
    failed: bool,
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
            failed: false,
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
        // 发起新搜索：清空旧结果，避免上一关键词/类型的列表残留误导（K4）。
        self.songs = Arc::new(Vec::new());
        self.albums = Arc::new(Vec::new());
        self.artists = Arc::new(Vec::new());
        self.playlists = Arc::new(Vec::new());
        self.selected = None;
        self.failed = false;
        self.status = format!("搜索中：{keyword}").into();
        cx.notify();

        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let value = siren_core::network::api::cloud_search(&client, &keyword, kind.api_value(), 30).await;
            let _ = tx.send(value);
        });

        cx.spawn(async move |this, cx| {
            let value = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.run_gen != gen {
                    return;
                }
                // 区分「网络/接口失败」与「空结果」：失败给出可重试提示，而非伪装成「0 条」（K4）。
                let value = match value {
                    Some(Ok(value)) => value,
                    Some(Err(err)) => {
                        log::warn!("搜索失败：{}", err.user_message());
                        this.failed = true;
                        this.status = format!("{} · 搜索失败", kind.label()).into();
                        cx.notify();
                        return;
                    }
                    None => return,
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

    /// 当前类型的结果条数。
    fn count(&self) -> usize {
        match self.kind {
            SearchKind::Song => self.songs.len(),
            SearchKind::Album => self.albums.len(),
            SearchKind::Artist => self.artists.len(),
            SearchKind::Playlist => self.playlists.len(),
        }
    }
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.weak_entity();
        let tabs = {
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .px(px(spacing::LG))
                .pt(px(spacing::XS));
            for kind in SearchKind::ALL {
                let active = kind == self.kind;
                let weak = weak.clone();
                row = row.child(
                    div()
                        .id(("search-kind", kind as usize))
                        .px(px(spacing::MD))
                        .py(px(5.0))
                        .rounded_full()
                        .cursor_pointer()
                        // 选中态统一：surface_light 背景（白字），hover 只在非选中时生效。
                        .bg(if active {
                            theme::surface_light()
                        } else {
                            theme::surface()
                        })
                        .when(!active, |s| s.hover(|s| s.bg(theme::card_surface())))
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |this, cx| this.set_kind(kind, cx)).ok();
                        })
                        .child(div().text_sm().text_color(theme::text_primary()).child(kind.label())),
                );
            }
            row
        };

        let body: AnyElement = if self.keyword.is_empty() {
            // 初始态（未搜索）：给引导文案而非空壳列表。
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child("输入关键词并回车开始搜索"),
                )
                .into_any_element()
        } else if self.failed {
            // 失败态：错误 + 重试（不显示「0 条」，K4）。
            let weak_retry = cx.weak_entity();
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(spacing::MD))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child("搜索失败，请检查网络后重试"),
                )
                .child(Button::new("search-retry").label("重试").on_click(move |_ev, _w, cx| {
                    weak_retry.update(cx, |this, cx| this.run(cx)).ok();
                }))
                .into_any_element()
        } else if self.count() == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child("未找到相关结果，换个关键词试试"),
                )
                .into_any_element()
        } else {
            match self.kind {
                SearchKind::Song => self.song_list(window, cx),
                SearchKind::Album => self.album_list(window, cx),
                SearchKind::Artist => self.artist_list(window, cx),
                SearchKind::Playlist => self.playlist_list(window, cx),
            }
        };

        let content = div().v_flex().flex_1().min_w(px(0.0)).w_full().child(tabs).child(body);

        page_shell(format!("搜索 · {}", self.status), content.into_any_element())
    }
}

impl SearchPage {
    fn song_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let covers = self.services.covers.clone();
        let playback = self.playback.clone();
        // 当前播放行高亮。
        let current = self.playback.read(cx).current_song_id();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "search-songs",
            count,
            dim::ROW_H_SM,
            move |range, _window, cx| {
                range
                    .map(|i| {
                        let song = &songs[i];
                        let sid = song.id;
                        let weak = weak.clone();
                        song_row(
                            SongRow::new(
                                "search-song",
                                song,
                                &playback,
                                &covers,
                                weak.clone(),
                                cx,
                                move |_ev, _window, cx| {
                                    weak.update(cx, |this, cx| {
                                        let items = this.songs.as_ref().clone();
                                        this.selected = Some(i);
                                        this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                                    })
                                    .ok();
                                },
                            )
                            .row_h(dim::ROW_H_SM)
                            .pad(spacing::LG)
                            // 序号列固定槽位：播放中显示 ▶、选中行用强调色，其余灰色。
                            .index(IndexCell::search(i, current == Some(sid), selected == Some(i)))
                            // 搜索页没有时长列。
                            .show_duration(false)
                            .add_to_playlist(true),
                        )
                    })
                    .collect::<Vec<_>>()
            },
            window,
            cx,
        )
        .into_any_element()
    }

    fn album_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let items = self.albums.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let render = smooth_virtual_list(
            "search-albums",
            count,
            60.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let album = &items[i];
                        let cover = covers.image(&album.cover_url, dim::COVER_LIST);
                        let weak = weak.clone();
                        let id = album.id;
                        let name = album.name.clone();
                        div()
                            .id(format!("search-album-{}", album.id))
                            .w_full()
                            .h(px(60.0))
                            .px(px(spacing::LG))
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .rounded(px(theme::RADIUS_SM))
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
            },
            window,
            cx,
        );
        render.into_any_element()
    }

    fn artist_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let items = self.artists.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let render = smooth_virtual_list(
            "search-artists",
            count,
            56.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let artist = &items[i];
                        let cover = covers.image_round(&artist.cover_url, 44.0);
                        let weak = weak.clone();
                        let id = artist.id;
                        let name = artist.name.clone();
                        div()
                            .id(format!("search-artist-{}", artist.id))
                            .w_full()
                            .h(px(dim::ROW_H_LIST))
                            .px(px(spacing::LG))
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .rounded(px(theme::RADIUS_SM))
                            .hover(|s| s.bg(theme::card_surface()))
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |_page, cx| {
                                    cx.emit(PageEvent::OpenArtist { id, name: name.clone() });
                                })
                                .ok();
                            })
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
            },
            window,
            cx,
        );
        render.into_any_element()
    }

    fn playlist_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let items = self.playlists.clone();
        let count = items.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let render = smooth_virtual_list(
            "search-playlists",
            count,
            60.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let pl = &items[i];
                        let cover = covers.image(&pl.cover_url, dim::COVER_LIST);
                        let weak = weak.clone();
                        let id = pl.id;
                        let name = pl.name.clone();
                        div()
                            .id(format!("search-playlist-{id}"))
                            .w_full()
                            .h(px(60.0))
                            .px(px(spacing::LG))
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .rounded(px(theme::RADIUS_SM))
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
            },
            window,
            cx,
        );
        render.into_any_element()
    }
}
