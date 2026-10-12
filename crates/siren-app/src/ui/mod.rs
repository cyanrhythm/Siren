//! GPUI 界面。
//!
//! 当前为可运行骨架：搜索 + 结果列表 + 播放条，串起 `siren-core` 的
//! 异步网络层与 [`siren_core::player::Player`]。
//!
//! ## 结构
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

use siren_core::model::Song;
use siren_core::network::client::NeteaseClient;
use siren_core::player::{Mpv, PlayMode, PlaybackController, PlaybackState, Player, SilentEngine};
use siren_platform::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlatformMediaSession, PlaybackStatus,
};

use crate::theme;

/// 主窗口根组件。
pub struct Root {
    runtime: tokio::runtime::Runtime,
    client: Arc<NeteaseClient>,
    player: Player,
    search_input: Entity<InputState>,
    songs: Arc<Vec<Song>>,
    status: SharedString,
    selected: Option<usize>,
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
                search_input,
                songs: Arc::new(Vec::new()),
                status: "回车搜索".into(),
                selected: None,
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
            this.start_search(cx);
        });
        entity
    }

    /// 每 500ms 驱动播放器刷新并同步 UI。
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
            // 自然播完后切歌：解析地址并播放
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

    /// 处理系统媒体指令。
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

    /// 把当前播放状态推送到系统媒体控制。
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

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let keyword = self.search_input.read(cx).value().trim().to_string();
        if keyword.is_empty() {
            return;
        }
        self.status = format!("搜索中：{keyword}").into();
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
                let items = songs.clone();
                this.songs = Arc::new(songs);
                this.selected = None;
                this.status = format!("搜索到 {} 首", this.songs.len()).into();
                if let Some(first) = this.player.set_queue(items) {
                    this.resolve_and_play(first, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 点击结果行：选中并解析播放。
    fn select_song(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(song) = self.songs.get(index).cloned() else {
            return;
        };
        self.selected = Some(index);
        if let Some(target) = self.player.play_at(index) {
            self.resolve_and_play(target, cx);
        } else {
            self.resolve_and_play(song, cx);
        }
    }

    /// 异步解析播放地址后交给播放器加载。
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
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let state = self.player.state();
        let weak = cx.weak_entity();

        let header = div().h(px(64.0)).w_full().flex().items_center().justify_center().child(
            div()
                .w(px(520.0))
                .h(px(44.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(14.0))
                .rounded(px(22.0))
                .bg(theme::surface())
                .child(icon(IconName::Search, 18.0, theme::text_gray()))
                .child(div().flex_1().child(Input::new(&self.search_input))),
        );

        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let weak_list = weak.clone();
        let list = uniform_list("results", count, move |range, _window, _cx| {
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
                        .bg(theme::window_bg())
                        .hover(|s| s.bg(theme::card_surface()))
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |this, cx| this.select_song(i, cx)).ok();
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

        let playing = state.playing;
        let title = state
            .song
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未在播放".to_string());
        let artist = state.song.as_ref().map(|s| s.artists.clone()).unwrap_or_default();
        let pos_label = fmt_time(state.position_ms);
        let dur_label = fmt_time(state.duration_ms);
        let vol_icon = IconName::Volume2;
        let progress_w = (vw - 300.0 - 260.0 - 64.0).max(160.0);

        let weak_play = weak.clone();
        let weak_next = weak;
        let player_bar = div()
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
                            .child(icon_button("prev", IconName::SkipBack, 32.0, true))
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
                    .child(icon(vol_icon, 18.0, theme::text_gray()))
                    .child(div().w(px(120.0)).child(Slider::new(&self.volume_slider))),
            );

        div()
            .v_flex()
            .size_full()
            .bg(theme::window_bg())
            .child(header)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .w_full()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(list),
            )
            .child(
                div().w_full().flex().items_center().justify_center().child(
                    div()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child(self.status.clone()),
                ),
            )
            .child(player_bar)
    }
}

fn fmt_time(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn icon(name: IconName, size: f32, color: Rgba) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

fn icon_button(id: &'static str, name: IconName, size: f32, _enabled: bool) -> impl IntoElement {
    div()
        .id(id)
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface()))
        .child(icon(name, 18.0, theme::text_gray()))
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
