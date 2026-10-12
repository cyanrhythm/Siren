//! GPUI 界面。
//!
//! 多页面骨架：侧栏导航 + 首页（推荐歌单）/ 音乐库 / 搜索 / 歌单详情 + 播放条，
//! 串起 `siren-core` 的异步网络层与 [`siren_core::player::Player`]。
//!
//! - `pages/`：页面级组件（首页、搜索、歌单、设置等）
//! - `components/`：可复用组件（播放条、曲目行、封面等）

pub mod components;
pub mod pages;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState, SliderValue};
use gpui_kit::component::Icon;
use gpui_kit::*;

use siren_core::model::{Playlist, PlaylistDetail, Song};
use siren_core::network::client::NeteaseClient;
use siren_core::player::{Mpv, PlayMode, PlaybackController, PlaybackState, Player, SilentEngine};
use siren_platform::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlatformMediaSession, PlaybackStatus,
};

use crate::theme;

/// 当前页面。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Home,
    Library,
    Search,
    Playlist { id: i64, name: String },
}

/// 主窗口根组件。
pub struct Root {
    runtime: tokio::runtime::Runtime,
    client: Arc<NeteaseClient>,
    player: Player,
    nav: Vec<Route>,

    // 首页/音乐库
    playlists: Arc<Vec<Playlist>>,
    playlists_status: SharedString,

    // 搜索
    search_input: Entity<InputState>,
    songs: Arc<Vec<Song>>,
    selected: Option<usize>,
    search_status: SharedString,

    // 歌单详情
    playlist: Option<Arc<PlaylistDetail>>,

    // 播放
    status: SharedString,
    mirrored: PlaybackState,
    seeking: bool,
    progress_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    media: Option<PlatformMediaSession>,
    media_cmds: std::sync::mpsc::Receiver<MediaCommand>,

    _search_sub: Subscription,
    _progress_sub: Subscription,
    _volume_sub: Subscription,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, runtime: tokio::runtime::Runtime) -> Entity<Self> {
        siren_platform::prepare_native_environment();
        let cookie = siren_core::auth::load_cookie();
        let client = Arc::new(NeteaseClient::with_cookie(cookie));
        let engine: Arc<dyn PlaybackController> = match Mpv::load(&siren_platform::mpv_library_candidates()) {
            Ok(mpv) => Arc::new(mpv),
            Err(_) => Arc::new(SilentEngine),
        };
        let player = Player::new(engine);

        // 系统媒体控制：从 GPUI 窗口取原生句柄（Windows SMTC 需要）。
        let hwnd = siren_platform::window_hwnd(window);
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<MediaCommand>();
        let cmd_tx = std::sync::Mutex::new(cmd_tx);
        let media = PlatformMediaSession::new(
            MediaSessionConfig {
                identity: "Siren".to_string(),
                desktop_entry: Some("com.cyanrhythm.Siren".to_string()),
                app_media_id: Some("com.cyanrhythm.Siren".to_string()),
                track_id_prefix: Some("/com/cyanrhythm/Siren/track".to_string()),
                hwnd,
            },
            Box::new(move |cmd| {
                if let Ok(tx) = cmd_tx.lock() {
                    let _ = tx.send(cmd);
                }
            }),
        )
        .ok();
        if media.is_some() {
            eprintln!("[siren] 系统媒体控制已启用");
        }

        let entity = cx.new(|cx| {
            let search_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("搜索歌曲、歌手…")
                    .default_value("周杰伦")
            });
            let _search_sub = cx.subscribe(&search_input, |this: &mut Root, _input, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.start_search(cx);
                }
            });

            let progress_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.001).default_value(0.0));
            let _progress_sub =
                cx.subscribe(
                    &progress_slider,
                    |this: &mut Root, _slider, event: &SliderEvent, cx| match event {
                        SliderEvent::Change(SliderValue::Single(f)) => {
                            this.seeking = true;
                            this.seek_fraction(*f, cx);
                        }
                        SliderEvent::Release(SliderValue::Single(f)) => {
                            this.seek_fraction(*f, cx);
                            this.seeking = false;
                        }
                        _ => {}
                    },
                );

            let volume_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.01).default_value(0.8));
            let _volume_sub = cx.subscribe(&volume_slider, |this: &mut Root, _slider, event: &SliderEvent, cx| {
                if let SliderEvent::Change(SliderValue::Single(f)) = event {
                    this.player.set_volume(*f as f64 * 100.0);
                    cx.notify();
                }
            });

            Self {
                runtime,
                client,
                player,
                nav: vec![Route::Home],
                playlists: Arc::new(Vec::new()),
                playlists_status: "加载中…".into(),
                search_input,
                songs: Arc::new(Vec::new()),
                selected: None,
                search_status: "回车搜索".into(),
                playlist: None,
                status: "就绪".into(),
                mirrored: PlaybackState::default(),
                seeking: false,
                progress_slider,
                volume_slider,
                media,
                media_cmds: cmd_rx,
                _search_sub,
                _progress_sub,
                _volume_sub,
            }
        });

        entity.update(cx, |this, cx| {
            this.start_tick_timer(window, cx);
            this.load_playlists(cx);
        });
        entity
    }

    fn route(&self) -> Route {
        self.nav.last().cloned().unwrap_or(Route::Home)
    }

    fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        self.nav.truncate(1);
        if let Route::Home = route {
        } else {
            self.nav.push(route);
        }
        if matches!(self.route(), Route::Home) && self.playlists.is_empty() {
            self.load_playlists(cx);
        }
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.len() > 1 {
            self.nav.pop();
            cx.notify();
        }
    }

    fn start_tick_timer(&mut self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(song) = self.player.tick() {
            self.resolve_and_play(song, cx);
        }
        let state = self.player.state();
        if !self.seeking {
            let frac = if state.duration_ms > 0 {
                state.position_ms as f32 / state.duration_ms as f32
            } else {
                0.0
            };
            self.progress_slider
                .update(cx, |s, cx| s.set_value(frac.clamp(0.0, 1.0), window, cx));
        }
        if state != self.mirrored {
            self.mirrored = state;
            cx.notify();
        }
        while let Ok(command) = self.media_cmds.try_recv() {
            self.apply_media_command(command, cx);
        }
        self.push_media_state();
    }

    // ---------- 数据加载 ----------

    fn load_playlists(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let list = siren_core::network::api::personalized_playlists(&client, 30)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        self.playlists_status = "加载推荐歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.playlists_status = format!("推荐歌单 {} 个", list.len()).into();
                this.playlists = Arc::new(list);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Playlist { id, name });
        self.playlist = None;
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
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
                this.playlist = detail.map(Arc::new);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let keyword = self.search_input.read(cx).value().trim().to_string();
        if keyword.is_empty() {
            return;
        }
        self.search_status = format!("搜索中：{keyword}").into();
        cx.notify();

        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let songs = siren_core::network::api::search(&client, &keyword, 1, 50)
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });

        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.search_status = format!("搜索到 {} 首", songs.len()).into();
                this.songs = Arc::new(songs);
                this.selected = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 用给定的歌曲列表建立队列并播放指定下标。
    fn play_from(&mut self, items: Vec<Song>, index: usize, cx: &mut Context<Self>) {
        self.selected = Some(index);
        if let Some(song) = self.player.set_queue(items) {
            let target = if index == 0 {
                song
            } else if let Some(s) = self.player.play_at(index) {
                s
            } else {
                song
            };
            self.resolve_and_play(target, cx);
        }
    }

    fn resolve_and_play(&mut self, song: Song, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let id = song.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let url = match siren_core::network::api::song_url_eapi(&client, id, "exhigh")
                .await
                .ok()
                .flatten()
            {
                Some(url) => Some(url),
                None => siren_core::network::api::song_url(&client, id).await.ok().flatten(),
            };
            let _ = tx.send(url);
        });

        self.status = format!("解析播放地址：{}", song.name).into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let url = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                match url {
                    Some(url) => {
                        this.player.play_url(&url);
                        this.status = format!("播放中：{}", song.name).into();
                    }
                    None => this.status = "无可用播放地址（需登录/会员）".into(),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let state = self.player.state();
        if state.duration_ms <= 0 {
            return;
        }
        let ms = (fraction.clamp(0.0, 1.0) as f64 * state.duration_ms as f64) as i64;
        self.player.seek_ms(ms);
        cx.notify();
    }

    fn next_track(&mut self, cx: &mut Context<Self>) {
        if let Some(song) = self.player.next_track() {
            self.resolve_and_play(song, cx);
        }
    }

    fn apply_media_command(&mut self, command: MediaCommand, cx: &mut Context<Self>) {
        match command {
            MediaCommand::Play => self.player.play(),
            MediaCommand::Pause | MediaCommand::Stop => self.player.pause(),
            MediaCommand::Toggle => self.player.toggle(),
            MediaCommand::Next => {
                self.next_track(cx);
                return;
            }
            MediaCommand::Previous => {
                if let Some(song) = self.player.previous_track() {
                    self.resolve_and_play(song, cx);
                }
                return;
            }
            MediaCommand::SeekTo(ms) => self.player.seek_ms(ms as i64),
            MediaCommand::SeekBy(delta) => {
                let target = self.player.state().position_ms + delta;
                self.player.seek_ms(target);
            }
            MediaCommand::SetVolume(v) => self.player.set_volume(v * 100.0),
            MediaCommand::SetShuffle(on) => {
                if on {
                    self.player.set_mode(PlayMode::Shuffle);
                }
            }
            MediaCommand::SetRepeat(one) => {
                self.player
                    .set_mode(if one { PlayMode::SingleLoop } else { PlayMode::ListLoop });
            }
        }
        cx.notify();
    }

    fn push_media_state(&self) {
        let Some(media) = &self.media else {
            return;
        };
        let state = self.player.state();
        let metadata = match &state.song {
            Some(song) => MediaMetadata {
                id: song.id.to_string(),
                title: song.name.clone(),
                artists: vec![song.artists.clone()],
                album: song.album.clone(),
                artwork_url: None,
                url: None,
                duration_ms: (state.duration_ms > 0).then_some(state.duration_ms as u64),
            },
            None => MediaMetadata::default(),
        };
        let status = if state.playing {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };
        media.set_metadata(&metadata);
        media.set_playback_status(status);
        media.set_timeline(state.position_ms.max(0) as u64, state.duration_ms.max(0) as u64);
        media.set_mode(state.mode);
    }

    // ---------- 渲染 ----------

    fn sidebar(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let current = self.route();
        let nav_item =
            |id: &'static str, icon_name: IconName, label: &'static str, route: Route, weak: WeakEntity<Self>| {
                let active = matches!(
                    (&current, &route),
                    (Route::Home, Route::Home) | (Route::Library, Route::Library) | (Route::Search, Route::Search)
                );
                div()
                    .id(id)
                    .w_full()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .bg(if active { theme::surface() } else { theme::pane() })
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |this, cx| this.go(route.clone(), cx)).ok();
                    })
                    .child(icon(
                        icon_name,
                        18.0,
                        if active {
                            theme::text_primary()
                        } else {
                            theme::text_gray()
                        },
                    ))
                    .child(div().text_sm().text_color(theme::text_primary()).child(label))
            };

        let weak_home = weak.clone();
        let weak_library = weak.clone();
        let weak_search = weak.clone();
        div()
            .v_flex()
            .w(px(210.0))
            .min_w(px(210.0))
            .h_full()
            .flex_none()
            .flex_shrink_0()
            .bg(theme::pane())
            .p(px(12.0))
            .gap(px(4.0))
            .child(
                div()
                    .px(px(8.0))
                    .py(px(10.0))
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::accent())
                    .child("Siren"),
            )
            .child(nav_item("nav-home", IconName::House, "首页", Route::Home, weak_home))
            .child(nav_item(
                "nav-library",
                IconName::Heart,
                "音乐库",
                Route::Library,
                weak_library,
            ))
            .child(nav_item(
                "nav-search",
                IconName::Search,
                "搜索",
                Route::Search,
                weak_search,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .px(px(8.0))
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(self.status.clone()),
            )
    }

    fn page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        match self.route() {
            Route::Home => self.home_page(weak).into_any_element(),
            Route::Library => self.library_page().into_any_element(),
            Route::Search => self.search_page(weak).into_any_element(),
            Route::Playlist { name, .. } => self.playlist_page(name, weak).into_any_element(),
        }
    }

    fn home_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let playlists = self.playlists.clone();
        let count = playlists.len();
        let weak_list = weak;
        let list = uniform_list("home-playlists", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let pl = &playlists[i];
                    let weak = weak_list.clone();
                    let id = pl.id;
                    let name = pl.name.clone();
                    div()
                        .id(i)
                        .w_full()
                        .h(px(64.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .rounded(px(8.0))
                        .hover(|s| s.bg(theme::card_surface()))
                        .cursor_pointer()
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |this, cx| {
                                this.open_playlist(id, name.clone(), cx);
                            })
                            .ok();
                        })
                        .child(cover_placeholder(48.0))
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

        page_shell(format!("首页 · {}", self.playlists_status), list.into_any_element())
    }

    fn library_page(&self) -> impl IntoElement {
        page_shell(
            "音乐库".to_string(),
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .v_flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child("登录后查看你的歌单、收藏与最近播放"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child("（Cookie 登录待接入界面）"),
                        ),
                )
                .into_any_element(),
        )
    }

    fn search_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let weak_list = weak;
        let list = uniform_list("search-results", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let song = &songs[i];
                    let marker = if selected == Some(i) { "▶ " } else { "   " };
                    let weak = weak_list.clone();
                    div()
                        .id(i)
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
                                this.play_from(items, i, cx);
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

        page_shell(
            format!("搜索 · {}", self.search_status),
            div()
                .v_flex()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div().p(px(12.0)).child(
                        div()
                            .w(px(420.0))
                            .h(px(40.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(14.0))
                            .rounded(px(20.0))
                            .bg(theme::surface())
                            .child(icon(IconName::Search, 18.0, theme::text_gray()))
                            .child(div().flex_1().child(Input::new(&self.search_input))),
                    ),
                )
                .child(list)
                .into_any_element(),
        )
    }

    fn playlist_page(&self, name: String, weak: WeakEntity<Self>) -> impl IntoElement {
        let weak_back = weak.clone();
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
                        weak_back.update(cx, |this, cx| this.back(cx)).ok();
                    })
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .child(div().text_lg().text_color(theme::text_primary()).child(name));

        let body: AnyElement = match &self.playlist {
            Some(detail) => {
                let tracks = detail.tracks.clone();
                let count = tracks.len();
                let selected = self.selected;
                let weak_list = weak.clone();
                uniform_list("playlist-tracks", count, move |range, _window, _cx| {
                    range
                        .map(|i| {
                            let song = &tracks[i];
                            let marker = if selected == Some(i) { "▶ " } else { "   " };
                            let weak = weak_list.clone();
                            div()
                                .id(i)
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
                                        let items =
                                            this.playlist.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        this.play_from(items, i, cx);
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

    fn player_bar(&self, weak: WeakEntity<Self>, vw: f32) -> impl IntoElement {
        let state = self.player.state();
        let playing = state.playing;
        let title = state
            .song
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未在播放".to_string());
        let artist = state.song.as_ref().map(|s| s.artists.clone()).unwrap_or_default();
        let pos_label = fmt_time(state.position_ms);
        let dur_label = fmt_time(state.duration_ms);
        let progress_w = (vw - 210.0 - 300.0 - 240.0 - 64.0).max(160.0);

        let weak_play = weak.clone();
        let weak_next = weak;
        div()
            .w_full()
            .h(px(theme::PLAYER_BAR_H))
            .flex()
            .items_center()
            .gap(px(16.0))
            .px(px(16.0))
            .child(
                div()
                    .w(px(300.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(cover_placeholder(56.0))
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
                                    .child(title),
                            )
                            .child(div().text_xs().text_color(theme::text_gray()).truncate().child(artist)),
                    ),
            )
            .child(
                div()
                    .w(px(progress_w + 120.0))
                    .flex_none()
                    .v_flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.0))
                            .child(icon(IconName::SkipBack, 22.0, theme::text_primary()))
                            .child(
                                div()
                                    .id("play")
                                    .size(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(theme::text_primary())
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak_play
                                            .update(cx, |this, cx| {
                                                this.player.toggle();
                                                cx.notify();
                                            })
                                            .ok();
                                    })
                                    .child(icon(
                                        if playing { IconName::Pause } else { IconName::Play },
                                        18.0,
                                        theme::pane(),
                                    )),
                            )
                            .child(
                                div()
                                    .id("next")
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak_next.update(cx, |this, cx| this.next_track(cx)).ok();
                                    })
                                    .child(icon(IconName::SkipForward, 22.0, theme::text_primary())),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(40.0))
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(pos_label),
                            )
                            .child(div().w(px(progress_w)).child(Slider::new(&self.progress_slider)))
                            .child(
                                div()
                                    .w(px(40.0))
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(dur_label),
                            ),
                    ),
            )
            .child(
                div()
                    .w(px(240.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .child(icon(IconName::Volume2, 18.0, theme::text_gray()))
                    .child(div().w(px(120.0)).child(Slider::new(&self.volume_slider))),
            )
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let weak = cx.weak_entity();
        div()
            .v_flex()
            .size_full()
            .bg(theme::window_bg())
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .overflow_hidden()
                    .child(self.sidebar(weak.clone()))
                    .child(self.page(weak.clone())),
            )
            .child(self.player_bar(weak, vw))
    }
}

fn page_shell(title: String, body: AnyElement) -> impl IntoElement {
    page_shell_with_header(
        div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(title)
            .into_any_element(),
        body,
    )
}

fn page_shell_with_header(header: AnyElement, body: AnyElement) -> impl IntoElement {
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .bg(theme::pane())
        .child(div().w_full().p(px(12.0)).child(header))
        .child(body)
}

fn fmt_time(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn icon(name: IconName, size: f32, color: Rgba) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

fn cover_placeholder(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .bg(theme::cover_placeholder())
        .child(icon(IconName::Music, size * 0.36, theme::text_gray()))
}
