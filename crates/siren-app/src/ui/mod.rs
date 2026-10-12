//! GPUI 界面。
//!
//! 多页面骨架：侧栏导航 + 首页（推荐歌单）/ 音乐库 / 搜索 / 歌单详情 + 播放条，
//! 串起 `siren-core` 的异步网络层与 [`siren_core::player::Player`]。
//!
//! ## 模块组织（按能力）
//!
//! - [`pages`]：页面级视图（首页、音乐库、搜索、歌单详情）
//! - [`components`]：可复用组件（侧栏、播放条）
//! - [`covers`]：封面加载器
//!
//! 视图方法以 `impl Root` 分文件承载，状态集中在 [`Root`]。

pub mod components;
pub mod covers;
pub mod pages;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::Icon;
use gpui_kit::*;

use siren_core::model::{Playlist, PlaylistDetail, Song};
use siren_core::network::client::NeteaseClient;
use siren_core::player::{Mpv, PlayMode, PlaybackController, PlaybackState, Player, SilentEngine};
use siren_platform::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlatformMediaSession, PlaybackStatus,
};

use crate::theme;
use covers::CoverLoader;

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

    fn page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        match self.route() {
            Route::Home => self.home_page(weak).into_any_element(),
            Route::Library => self.library_page(weak).into_any_element(),
            Route::Search => self.search_page(weak).into_any_element(),
            Route::Playlist { name, .. } => self.playlist_page(name, weak).into_any_element(),
        }
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

// ---------- 共享小部件 ----------

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
