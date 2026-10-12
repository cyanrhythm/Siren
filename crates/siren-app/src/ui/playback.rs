//! 播放服务：封装核心 [`Player`] + 网络音源解析 + 系统媒体控制，作为共享 Entity。
//!
//! 界面（`Root` 及各页面）通过 `Entity<Playback>` 订阅/驱动它，而不直接触碰播放细节。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::*;

use siren_core::model::{AudioQuality, Song};
use siren_core::network::client::NeteaseClient;
use siren_core::player::{Mpv, PlayMode, PlaybackController, PlaybackState, Player, SilentEngine};
use siren_platform::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlatformMediaSession, PlaybackStatus,
};

/// 播放服务。
pub(crate) struct Playback {
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
    player: Player,
    status: SharedString,
    mirrored: PlaybackState,
    seeking: bool,
    /// 当前音质档位（持久化于 `siren-core::settings`）。
    quality: AudioQuality,
    /// 已喜欢的歌曲 ID 集合。
    liked: HashSet<i64>,
    /// 正在播放的歌曲与起始时刻（用于打卡上报时长）。
    playing_id: Option<i64>,
    playing_at: Option<Instant>,
    progress_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    media: Option<PlatformMediaSession>,
    media_cmds: std::sync::mpsc::Receiver<MediaCommand>,
    _progress_sub: Subscription,
    _volume_sub: Subscription,
}

impl Playback {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut App,
        client: Arc<NeteaseClient>,
        handle: tokio::runtime::Handle,
    ) -> Entity<Self> {
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
            let progress_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.001).default_value(0.0));
            let _progress_sub = cx.subscribe(
                &progress_slider,
                |this: &mut Playback, _slider, event: &SliderEvent, cx| match event {
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
            let _volume_sub = cx.subscribe(
                &volume_slider,
                |this: &mut Playback, _slider, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(SliderValue::Single(f)) = event {
                        this.player.set_volume(*f as f64 * 100.0);
                        cx.notify();
                    }
                },
            );
            Self {
                client,
                handle,
                player,
                status: "就绪".into(),
                mirrored: PlaybackState::default(),
                seeking: false,
                quality: siren_core::settings::load_quality(),
                liked: HashSet::new(),
                playing_id: None,
                playing_at: None,
                progress_slider,
                volume_slider,
                media,
                media_cmds: cmd_rx,
                _progress_sub,
                _volume_sub,
            }
        });
        entity.update(cx, |this, cx| this.start_tick_timer(window, cx));
        entity
    }

    pub(crate) fn state(&self) -> PlaybackState {
        self.player.state()
    }

    /// 当前音质档位。
    pub(crate) fn quality(&self) -> AudioQuality {
        self.quality
    }

    /// 设置音质档位（持久化）。
    pub(crate) fn set_quality(&mut self, quality: AudioQuality, cx: &mut Context<Self>) {
        self.quality = quality;
        siren_core::settings::save_quality(quality);
        cx.notify();
    }

    /// 当前歌曲是否已喜欢。
    pub(crate) fn current_liked(&self) -> bool {
        self.player
            .state()
            .song
            .map(|s| self.liked.contains(&s.id))
            .unwrap_or(false)
    }

    /// 载入已喜欢歌曲 ID（登录后）。
    pub(crate) fn load_likes(&mut self, uid: i64, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let ids = siren_core::network::api::liked_song_ids(&client, uid)
                .await
                .unwrap_or_default();
            let _ = tx.send(ids);
        });
        cx.spawn(async move |this, cx| {
            let ids = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.liked = ids.into_iter().collect();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 清空喜欢状态（退出登录）。
    pub(crate) fn clear_likes(&mut self, cx: &mut Context<Self>) {
        self.liked.clear();
        cx.notify();
    }

    /// 喜欢 / 取消喜欢当前歌曲（乐观更新 + 失败回滚）。
    pub(crate) fn toggle_like_current(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.player.state().song else {
            return;
        };
        let id = song.id;
        let like = !self.liked.contains(&id);
        if like {
            self.liked.insert(id);
        } else {
            self.liked.remove(&id);
        }
        self.status = if like {
            format!("已喜欢：{}", song.name).into()
        } else {
            format!("已取消喜欢：{}", song.name).into()
        };
        cx.notify();

        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let ok = siren_core::network::api::like_song(&client, id, like).await.is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            if !ok {
                this.update(cx, |this, cx| {
                    // 回滚
                    if like {
                        this.liked.remove(&id);
                    } else {
                        this.liked.insert(id);
                    }
                    this.status = "喜欢操作失败".into();
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn progress_slider(&self) -> &Entity<SliderState> {
        &self.progress_slider
    }

    pub(crate) fn volume_slider(&self) -> &Entity<SliderState> {
        &self.volume_slider
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

    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        self.player.toggle();
        cx.notify();
    }

    /// 用给定歌曲列表建立队列并播放指定下标。
    pub(crate) fn play_from(&mut self, items: Vec<Song>, index: usize, cx: &mut Context<Self>) {
        if let Some(song) = self.player.set_queue(items) {
            let target = if index == 0 {
                song
            } else {
                self.player.play_at(index).unwrap_or(song)
            };
            self.resolve_and_play(target, cx);
        }
    }

    pub(crate) fn next_track(&mut self, cx: &mut Context<Self>) {
        if let Some(song) = self.player.next_track() {
            self.resolve_and_play(song, cx);
        }
    }

    pub(crate) fn previous_track(&mut self, cx: &mut Context<Self>) {
        if let Some(song) = self.player.previous_track() {
            self.resolve_and_play(song, cx);
        }
    }

    /// 切换随机播放。
    pub(crate) fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        let mode = if self.player.mode() == PlayMode::Shuffle {
            PlayMode::ListLoop
        } else {
            PlayMode::Shuffle
        };
        self.player.set_mode(mode);
        cx.notify();
    }

    /// 切换单曲循环。
    pub(crate) fn toggle_repeat(&mut self, cx: &mut Context<Self>) {
        let mode = if self.player.mode() == PlayMode::SingleLoop {
            PlayMode::ListLoop
        } else {
            PlayMode::SingleLoop
        };
        self.player.set_mode(mode);
        cx.notify();
    }

    /// 打卡上报上一首的实际播放时长（切歌时）。
    fn finish_previous(&mut self) {
        if let (Some(prev), Some(at)) = (self.playing_id, self.playing_at) {
            let seconds = at.elapsed().as_secs() as i64;
            let client = self.client.clone();
            self.handle.spawn(async move {
                let _ = siren_core::network::api::report_play_end(&client, prev, prev, seconds).await;
            });
        }
    }

    fn resolve_and_play(&mut self, song: Song, cx: &mut Context<Self>) {
        // 打卡上报：结束上一首（按实际播放时长），并开始新一首。
        self.finish_previous();
        let client_report = self.client.clone();
        let song_id = song.id;
        self.playing_id = Some(song_id);
        self.playing_at = Some(Instant::now());
        self.handle.spawn(async move {
            let _ = siren_core::network::api::report_start_play(&client_report, song_id, song_id).await;
        });

        let client = self.client.clone();
        let id = song.id;
        let level = self.quality.code();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let url = match siren_core::network::api::song_url_eapi(&client, id, level)
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
                self.previous_track(cx);
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
}
