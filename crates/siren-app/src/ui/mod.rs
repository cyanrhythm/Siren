//! GPUI 界面。
//!
//! 多页面骨架：侧栏导航 + 首页（推荐歌单）/ 音乐库 / 搜索 / 歌单详情 + 播放条，
//! 串起 `siren-core` 的异步网络层与 [`siren_core::player::Player`]。
//!
//! - `pages/`：页面级组件（首页、搜索、歌单、设置等）
//! - `components/`：可复用组件（播放条、曲目行、封面等）

pub mod components;
pub mod pages;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
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

    // 音乐库 / 登录
    logged_in: bool,
    nickname: SharedString,
    login_input: Entity<InputState>,
    login_status: SharedString,
    my_playlists: Arc<Vec<Playlist>>,
    uid: Option<i64>,

    // 播放
    status: SharedString,
    mirrored: PlaybackState,
    seeking: bool,
    progress_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    covers: CoverLoader,
    media: Option<PlatformMediaSession>,
    media_cmds: std::sync::mpsc::Receiver<MediaCommand>,

    _search_sub: Subscription,
    _progress_sub: Subscription,
    _volume_sub: Subscription,
    _login_sub: Subscription,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, runtime: tokio::runtime::Runtime) -> Entity<Self> {
        siren_platform::prepare_native_environment();
        let cookie = std::env::var("SIREN_COOKIE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| siren_core::auth::normalize_cookie(&s))
            .or_else(siren_core::auth::load_cookie);
        let has_cookie = cookie.is_some();
        let client = Arc::new(NeteaseClient::with_cookie(cookie));
        let covers = CoverLoader::new(client.clone(), runtime.handle().clone());
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

        let initial_route = match std::env::var("SIREN_START").ok().as_deref() {
            Some("library") => Route::Library,
            Some("search") => Route::Search,
            _ => Route::Home,
        };

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

            let login_input = cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 MUSIC_U 或完整 Cookie"));
            let _login_sub = cx.subscribe(&login_input, |this: &mut Root, _input, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.do_login(cx);
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
                nav: vec![initial_route],
                playlists: Arc::new(Vec::new()),
                playlists_status: "加载中…".into(),
                search_input,
                songs: Arc::new(Vec::new()),
                selected: None,
                search_status: "回车搜索".into(),
                playlist: None,
                logged_in: false,
                nickname: "未登录".into(),
                login_input,
                login_status: if has_cookie {
                    "登录校验中…".into()
                } else {
                    "未登录".into()
                },
                my_playlists: Arc::new(Vec::new()),
                uid: None,
                status: "就绪".into(),
                mirrored: PlaybackState::default(),
                seeking: false,
                progress_slider,
                volume_slider,
                covers,
                media,
                media_cmds: cmd_rx,
                _search_sub,
                _progress_sub,
                _volume_sub,
                _login_sub,
            }
        });

        entity.update(cx, |this, cx| {
            this.start_tick_timer(window, cx);
            this.load_playlists(cx);
            if has_cookie {
                this.refresh_account(cx);
            }
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

    fn do_login(&mut self, cx: &mut Context<Self>) {
        let raw = self.login_input.read(cx).value().trim().to_string();
        let Some(cookie) = siren_core::auth::normalize_cookie(&raw) else {
            self.login_status = "请输入 MUSIC_U 或完整 Cookie".into();
            cx.notify();
            return;
        };
        siren_core::auth::save_cookie(&cookie);
        self.client = Arc::new(NeteaseClient::with_cookie(Some(cookie)));
        self.login_status = "登录校验中…".into();
        cx.notify();
        self.refresh_account(cx);
    }

    fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let account = siren_core::network::api::account(&client).await.ok();
            let _ = tx.send(account);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let account = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                let profile = account.and_then(|v| {
                    siren_core::network::api::account_uid(&v)
                        .map(|uid| (uid, siren_core::network::api::account_nickname(&v)))
                });
                match profile {
                    Some((uid, nick)) => {
                        this.logged_in = true;
                        this.uid = Some(uid);
                        this.nickname = nick.unwrap_or_else(|| "已登录".to_string()).into();
                        this.login_status = format!("已登录：{}", this.nickname).into();
                        eprintln!("[siren] 已登录：{} (uid={uid})", this.nickname);
                        this.load_my_playlists(cx);
                    }
                    None => {
                        this.logged_in = false;
                        this.login_status = "未登录（Cookie 无效或已过期）".into();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_my_playlists(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.uid else {
            return;
        };
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let list = siren_core::network::api::user_playlists(&client, uid, 50, 0)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        self.login_status = "加载我的歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.login_status = format!("我的歌单 {} 个", list.len()).into();
                this.my_playlists = Arc::new(list);
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
            Route::Library => self.library_page(weak).into_any_element(),
            Route::Search => self.search_page(weak).into_any_element(),
            Route::Playlist { name, .. } => self.playlist_page(name, weak).into_any_element(),
        }
    }

    fn home_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let playlists = self.playlists.clone();
        let count = playlists.len();
        let covers = self.covers.clone();
        let weak_list = weak;
        let list = uniform_list("home-playlists", count, move |range, _window, cx| {
            range
                .map(|i| {
                    let pl = &playlists[i];
                    let cover = covers.image(&pl.cover_url, 48.0, &weak_list, cx);
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

        page_shell(format!("首页 · {}", self.playlists_status), list.into_any_element())
    }

    fn library_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        if self.logged_in {
            let playlists = self.my_playlists.clone();
            let count = playlists.len();
            let covers = self.covers.clone();
            let weak_list = weak;
            let list = uniform_list("lib-playlists", count, move |range, _window, cx| {
                range
                    .map(|i| {
                        let pl = &playlists[i];
                        let cover = covers.image(&pl.cover_url, 48.0, &weak_list, cx);
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

            page_shell(
                format!("音乐库 · {} · {}", self.nickname, self.login_status),
                list.into_any_element(),
            )
        } else {
            let weak_login = weak;
            let card = div()
                .v_flex()
                .gap(px(10.0))
                .w(px(440.0))
                .p(px(16.0))
                .rounded(px(10.0))
                .bg(theme::card_surface())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("登录后查看你的歌单"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child("粘贴 MUSIC_U 或完整 Cookie（也可用 SIREN_COOKIE 环境变量）"),
                )
                .child(Input::new(&self.login_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("login")
                                .primary()
                                .label("登录")
                                .on_click(move |_ev, _window, cx| {
                                    weak_login.update(cx, |this, cx| this.do_login(cx)).ok();
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(self.login_status.clone()),
                        ),
                );
            page_shell(
                "音乐库".to_string(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(card)
                    .into_any_element(),
            )
        }
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

/// 封面加载器：按尺寸缓存 + 后台抓取 + 通知重绘。
#[derive(Clone)]
struct CoverLoader {
    cache: Arc<Mutex<HashMap<String, Arc<Image>>>>,
    inflight: Arc<Mutex<HashSet<String>>>,
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
}

impl CoverLoader {
    fn new(client: Arc<NeteaseClient>, handle: tokio::runtime::Handle) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            client,
            handle,
        }
    }

    fn image(&self, url: &str, size: f32, weak: &WeakEntity<Root>, cx: &mut App) -> AnyElement {
        if url.is_empty() {
            return cover_placeholder(size).into_any_element();
        }
        let key = sized_url(url, size);
        if let Some(image) = self.cache.lock().unwrap().get(&key).cloned() {
            return img(image).w(px(size)).h(px(size)).rounded(px(6.0)).into_any_element();
        }
        self.request(key, weak, cx);
        cover_placeholder(size).into_any_element()
    }

    fn request(&self, key: String, weak: &WeakEntity<Root>, cx: &mut App) {
        if !self.inflight.lock().unwrap().insert(key.clone()) {
            return;
        }
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<Option<Arc<Image>>>();
        let fetch_key = key.clone();
        self.handle.spawn(async move {
            let image = match client.get_bytes(&fetch_key).await {
                Ok(bytes) => guess_format(&bytes).map(|fmt| Arc::new(Image::from_bytes(fmt, bytes))),
                Err(_) => None,
            };
            let _ = tx.send(image);
        });

        let cache = self.cache.clone();
        let inflight = self.inflight.clone();
        let key_for = key;
        weak.clone()
            .update(cx, move |_this, cx| {
                cx.spawn(async move |this, cx| {
                    let image = rx.await.ok().flatten();
                    inflight.lock().unwrap().remove(&key_for);
                    if let Some(image) = image {
                        cache.lock().unwrap().insert(key_for, image);
                        this.update(cx, |_this, cx| cx.notify()).ok();
                    }
                })
                .detach();
            })
            .ok();
    }
}

/// 网易云封面支持 `?param=WxH` 生成指定尺寸，减少下载量。
fn sized_url(url: &str, size: f32) -> String {
    if url.contains("126.net") && !url.contains("param=") {
        format!("{url}?param={}y{}", size as u32, size as u32)
    } else {
        url.to_string()
    }
}

/// 通过魔数猜测图片格式。
fn guess_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        Some(ImageFormat::Png)
    } else if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 6 && (&bytes[..6] == b"GIF89a" || &bytes[..6] == b"GIF87a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
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
