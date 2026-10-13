//! 播放服务：封装核心 [`Player`] + 网络音源解析 + 系统媒体控制，作为共享 Entity。
//!
//! 界面（`Root` 及各页面）通过 `Entity<Playback>` 订阅/驱动它，而不直接触碰播放细节。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::*;

use super::covers::CoverLoader;
use super::notifications::Notifications;
use super::settings_store::SettingsStore;
use siren_core::lyrics::LyricLine;
use siren_core::model::{Artist, ArtistAlbum, ArtistDetail, AudioQuality, CommentPage, Song, SongWiki};
use siren_core::network::client::NeteaseClient;
use siren_core::player::{
    Mpv, PlayMode, PlaySource, PlaySourceKind, PlaybackController, PlaybackState, Player, SilentEngine,
};
use siren_platform::{
    MediaCommand, MediaMetadata, MediaSession, MediaSessionConfig, PlatformMediaSession, PlaybackStatus,
};

/// 播放上下文标签（对齐 Melodia）。
pub(crate) const CONTEXT_INTELLIGENCE: &str = "intelligence";
pub(crate) const CONTEXT_ROAMING: &str = "similar_roaming";

/// 退出时同步落盘用的全局句柄（弱引用）。
pub(crate) struct PlaybackHandle(std::sync::Mutex<Option<WeakEntity<Playback>>>);

impl Global for PlaybackHandle {}

/// 退出时同步落盘播放会话（供 `lib.rs` 的关闭回调调用）。
pub(crate) fn flush_playback_session(cx: &mut App) {
    let weak = cx
        .try_global::<PlaybackHandle>()
        .and_then(|h| h.0.lock().ok().and_then(|g| g.clone()));
    if let Some(weak) = weak {
        let _ = weak.update(cx, |playback, _cx| playback.save_session_now());
    }
}

/// 播放服务。
pub(crate) struct Playback {
    client: Arc<NeteaseClient>,
    handle: tokio::runtime::Handle,
    covers: CoverLoader,
    notifications: Entity<Notifications>,
    player: Player,
    status: SharedString,
    mirrored: PlaybackState,
    seeking: bool,
    /// 已喜欢的歌曲 ID 集合。
    liked: HashSet<i64>,
    /// 当前播放歌曲的详情（含发行时间等，来自 `song_detail`）。
    detail: Option<Song>,
    /// 当前歌曲歌词。
    lyrics: Vec<LyricLine>,
    /// 当前歌曲百科/制作人。
    wiki: Option<SongWiki>,
    /// 当前歌曲评论（首页预览）。
    comments: CommentPage,
    /// 当前歌曲首位歌手的详情 / 专辑 / 相似 / 关注态。
    artist: Option<ArtistDetail>,
    artist_albums: Vec<ArtistAlbum>,
    similar_artists: Vec<Artist>,
    artist_followed: bool,
    artist_fans: i64,
    /// 上次已重绘的整数秒（用于外壳节流重绘）。
    last_sec: i64,
    /// 冷启动恢复的待播放进度（URL 加载后 seek 到此处后清除）。
    resume_ms: Option<i64>,
    /// 已恢复但尚未解析 URL（首次播放需先解析）。
    needs_resolve: bool,
    /// 上次进度落盘时刻（节流，5s）。
    last_progress_save: Instant,
    // 进度插值锚点（弥补 mpv time-pos 更新粒度）
    anchor_pos: i64,
    anchor_at: Instant,
    was_playing: bool,
    /// 正在播放的歌曲与起始时刻（用于打卡上报时长）。
    playing_id: Option<i64>,
    playing_at: Option<Instant>,
    /// 打卡上报的容器 id（歌单来源用歌单 id，其余用歌曲 id）。
    reporting_container: i64,
    /// 上一次已记录的本地最近播放歌单 id（同一歌单连续切歌不重复记录）。
    last_recorded_playlist: i64,
    /// 漫游续接：上一次已触发续接的歌曲 id（避免同曲反复拉取）。
    roaming_last_trigger: i64,
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
        covers: CoverLoader,
        notifications: Entity<Notifications>,
    ) -> Entity<Self> {
        let engine: Arc<dyn PlaybackController> = match Mpv::load(&siren_platform::mpv_library_candidates()) {
            Ok(mpv) => Arc::new(mpv),
            Err(_) => Arc::new(SilentEngine),
        };
        let mut player = Player::new(engine);
        // 冷启动恢复上次队列/当前曲/进度（不自动播放；首次播放时解析 URL 并 seek 回进度）
        let mut resume_ms: Option<i64> = None;
        let mut needs_resolve = false;
        if let Some(queue_session) = siren_core::playback_session::QueueSession::load() {
            let index = queue_session.index;
            let current_id = queue_session.queue.get(index).map(|s| s.id);
            // 进度仅当与恢复出的当前曲一致时才采用
            let (position_ms, duration_ms) = match siren_core::playback_session::ProgressSession::load() {
                Some(p) if Some(p.song_id) == current_id => (p.position_ms, p.duration_ms),
                _ => (0, 0),
            };
            if player
                .restore(
                    queue_session.queue,
                    index,
                    queue_session.mode,
                    position_ms,
                    duration_ms,
                    queue_session.play_source,
                    queue_session.play_context,
                )
                .is_some()
            {
                resume_ms = (position_ms > 0).then_some(position_ms);
                needs_resolve = true;
            }
        }

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
            log::info!("系统媒体控制已启用");
        }

        let entity = cx.new(|cx| {
            let progress_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.001).default_value(0.0));
            let _progress_sub = cx.subscribe(
                &progress_slider,
                |this: &mut Playback, _slider, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(SliderValue::Single(_f)) => {
                        // 拖动中只标记预览（由 Slider 自身显示位置），不发 seek，避免连续 seek 噪音
                        this.seeking = true;
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
                covers,
                notifications,
                player,
                status: "就绪".into(),
                mirrored: PlaybackState::default(),
                seeking: false,
                liked: HashSet::new(),
                detail: None,
                lyrics: Vec::new(),
                wiki: None,
                comments: CommentPage::default(),
                artist: None,
                artist_albums: Vec::new(),
                similar_artists: Vec::new(),
                artist_followed: false,
                artist_fans: 0,
                last_sec: 0,
                resume_ms,
                needs_resolve,
                last_progress_save: Instant::now(),
                anchor_pos: 0,
                anchor_at: Instant::now(),
                was_playing: false,
                playing_id: None,
                playing_at: None,
                reporting_container: 0,
                last_recorded_playlist: 0,
                roaming_last_trigger: 0,
                progress_slider,
                volume_slider,
                media,
                media_cmds: cmd_rx,
                _progress_sub,
                _volume_sub,
            }
        });
        entity.update(cx, |this, cx| this.start_tick_timer(window, cx));
        // 供退出时同步落盘（弱引用，不延长生命周期）
        cx.set_global(PlaybackHandle(std::sync::Mutex::new(Some(entity.downgrade()))));
        entity
    }

    pub(crate) fn state(&self) -> PlaybackState {
        self.player.state()
    }

    /// 当前队列来源容器（歌单/专辑）。
    pub(crate) fn source(&self) -> Option<&PlaySource> {
        self.player.source()
    }

    /// 指定歌曲是否已喜欢。
    pub(crate) fn is_liked(&self, song_id: i64) -> bool {
        self.liked.contains(&song_id)
    }

    /// 播放歌单：拉取详情后以歌单为来源起播。
    pub(crate) fn play_playlist(&mut self, id: i64, name: String, cover: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(
                siren_core::network::api::playlist_detail(&client, id)
                    .await
                    .ok()
                    .flatten(),
            );
        });
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| match detail {
                Some(d) if !d.tracks.is_empty() => {
                    let source = PlaySource {
                        kind: PlaySourceKind::Playlist,
                        id,
                        name,
                        cover_url: cover,
                    };
                    this.play_from_source(d.tracks, 0, Some(source), cx);
                }
                _ => this.notifications.update(cx, |n, cx| n.error("加载歌单失败", cx)),
            })
            .ok();
        })
        .detach();
    }

    /// 播放专辑：拉取详情后以专辑为来源起播。
    pub(crate) fn play_album(&mut self, id: i64, name: String, cover: String, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::album_detail(&client, id).await.ok().flatten());
        });
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| match detail {
                Some(d) if !d.songs.is_empty() => {
                    let source = PlaySource {
                        kind: PlaySourceKind::Album,
                        id,
                        name,
                        cover_url: cover,
                    };
                    this.play_from_source(d.songs, 0, Some(source), cx);
                }
                _ => this.notifications.update(cx, |n, cx| n.error("加载专辑失败", cx)),
            })
            .ok();
        })
        .detach();
    }

    /// 播放单首歌曲（按 id 拉取详情后起播）。
    pub(crate) fn play_song(&mut self, id: i64, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::song_detail(&client, id).await.ok().flatten());
        });
        cx.spawn(async move |this, cx| {
            let song = rx.await.ok().flatten();
            this.update(cx, |this, cx| match song {
                Some(song) => this.play_from(vec![song], 0, cx),
                None => this.notifications.update(cx, |n, cx| n.error("加载歌曲失败", cx)),
            })
            .ok();
        })
        .detach();
    }

    /// 把歌曲插到当前曲之后（「下一首播放」）；无当前曲则直接播放。
    pub(crate) fn play_next(&mut self, song: Song, cx: &mut Context<Self>) {
        if self.player.state().song.is_none() {
            self.play_from(vec![song], 0, cx);
            return;
        }
        self.player.insert_next(song.clone());
        self.save_queue_session();
        self.notifications
            .update(cx, |n, cx| n.success(format!("已加入下一首播放：{}", song.name), cx));
        cx.notify();
    }

    /// 喜欢 / 取消喜欢任意歌曲（乐观更新 + 回滚）。
    pub(crate) fn toggle_like_song(&mut self, song: Song, cx: &mut Context<Self>) {
        let id = song.id;
        let like = !self.liked.contains(&id);
        if like {
            self.liked.insert(id);
        } else {
            self.liked.remove(&id);
        }
        cx.notify();
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::like_song(&client, id, like).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(())) => {}
                    Some(Err(err)) => {
                        if like {
                            this.liked.remove(&id);
                        } else {
                            this.liked.insert(id);
                        }
                        this.notifications.update(cx, |n, cx| n.error(err.user_message(), cx));
                    }
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 当前播放上下文标签（心动/漫游等）。
    pub(crate) fn context(&self) -> Option<&str> {
        self.player.context()
    }

    /// 退出特殊上下文（心动/漫游）：还原备份队列；若当前曲变化则重新解析播放。
    pub(crate) fn exit_special_context(&mut self, cx: &mut Context<Self>) {
        let before = self.player.state().song.map(|s| s.id);
        self.player.exit_special_context();
        let after = self.player.state().song.map(|s| s.id);
        self.save_queue_session();
        if after.is_some() && after != before {
            if let Some(song) = self.player.state().song {
                self.resolve_and_play(song, cx);
                return;
            }
        }
        cx.notify();
    }

    /// 当前音质档位（唯一真源：设置 store）。
    pub(crate) fn quality(&self, cx: &App) -> AudioQuality {
        SettingsStore::of(cx).quality()
    }

    /// 设置音质档位（写入设置 store 并落盘）。
    pub(crate) fn set_quality(&mut self, quality: AudioQuality, cx: &mut Context<Self>) {
        cx.global_mut::<SettingsStore>().set_quality(quality);
        cx.notify();
    }

    /// 当前歌曲详情（含发行时间）。
    pub(crate) fn current_detail(&self) -> Option<&Song> {
        self.detail.as_ref()
    }

    /// 当前歌曲歌词。
    pub(crate) fn current_lyrics(&self) -> &[LyricLine] {
        &self.lyrics
    }

    /// 当前歌曲百科/制作人。
    pub(crate) fn current_wiki(&self) -> Option<&SongWiki> {
        self.wiki.as_ref()
    }

    /// 评论预览。
    pub(crate) fn comments(&self) -> &CommentPage {
        &self.comments
    }

    /// 当前歌手详情 / 专辑 / 相似 / 关注。
    pub(crate) fn artist(&self) -> Option<&ArtistDetail> {
        self.artist.as_ref()
    }
    pub(crate) fn artist_albums(&self) -> &[ArtistAlbum] {
        &self.artist_albums
    }
    pub(crate) fn similar_artists(&self) -> &[Artist] {
        &self.similar_artists
    }
    pub(crate) fn artist_followed(&self) -> bool {
        self.artist_followed
    }
    pub(crate) fn artist_fans(&self) -> i64 {
        self.artist_fans
    }

    /// 切换对当前歌手的关注。
    pub(crate) fn toggle_artist_follow(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.artist.as_ref().map(|a| a.id) else {
            return;
        };
        let follow = !self.artist_followed;
        self.artist_followed = follow;
        cx.notify();
        let client = self.client.clone();
        self.handle.spawn(async move {
            let _ = siren_core::network::api::artist_follow(&client, id, follow).await;
        });
    }

    /// 相似歌曲漫游：以当前歌曲的相似歌曲建队列并播放。
    pub(crate) fn play_similar_roaming(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.player.state().song else { return };
        let id = song.id;
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let songs = siren_core::network::api::simi_songs(&client, id)
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });
        self.status = "加载相似歌曲…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if songs.is_empty() {
                    this.status = "无相似歌曲".into();
                    cx.notify();
                    return;
                }
                this.player.take_snapshot();
                this.player.set_source(None);
                this.player.set_context(Some(CONTEXT_ROAMING.to_string()));
                this.roaming_last_trigger = 0;
                if let Some(first) = this.player.set_queue(songs) {
                    this.resolve_and_play(first, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 心动模式：拉取智能推荐队列并播放。
    pub(crate) fn start_intelligence(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.player.state().song else { return };
        let id = song.id;
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let songs = siren_core::network::api::intelligence_songs(&client, id, id, "fromPlayOne")
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });
        self.status = "加载心动模式…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if songs.is_empty() {
                    this.status = "无推荐歌曲".into();
                    cx.notify();
                    return;
                }
                this.player.take_snapshot();
                this.player.set_source(None);
                this.player.set_context(Some(CONTEXT_INTELLIGENCE.to_string()));
                if let Some(first) = this.player.set_queue(songs) {
                    this.resolve_and_play(first, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 是否启用系统媒体控制（唯一真源：设置 store）。
    pub(crate) fn media_keys_enabled(&self, cx: &App) -> bool {
        SettingsStore::of(cx).media_keys()
    }

    /// 设置系统媒体控制开关（写入设置 store 并落盘）；关闭时清空系统卡片。
    pub(crate) fn set_media_keys(&mut self, enabled: bool, cx: &mut Context<Self>) {
        cx.global_mut::<SettingsStore>().set_media_keys(enabled);
        if !enabled {
            if let Some(media) = &self.media {
                media.set_metadata(&MediaMetadata::default());
                media.set_playback_status(PlaybackStatus::Stopped);
            }
        }
        cx.notify();
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

    pub(crate) fn progress_slider(&self) -> &Entity<SliderState> {
        &self.progress_slider
    }

    pub(crate) fn volume_slider(&self) -> &Entity<SliderState> {
        &self.volume_slider
    }

    fn start_tick_timer(&mut self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(song) = self.player.tick() {
            log::debug!("收到下一首请求 id={}", song.id);
            self.resolve_and_play(song, cx);
        }
        let state = self.player.state();
        if !self.seeking {
            // 按墙钟插值：mpv 的 time-pos 更新粒度较粗，直接引用会呈阶跃；
            // 以读到的位置为锚点，两次读数之间用时间推演，得到连续的进度。
            let now = Instant::now();
            let playing = state.playing;
            if playing && self.was_playing {
                let elapsed = now.duration_since(self.anchor_at).as_millis() as i64;
                let predicted = self.anchor_pos + elapsed;
                // 与推演偏差过大（seek / 跳变）则重新锚定
                if (state.position_ms - predicted).abs() > 400 {
                    self.anchor_pos = state.position_ms;
                    self.anchor_at = now;
                }
            } else {
                self.anchor_pos = state.position_ms;
                self.anchor_at = now;
            }
            self.was_playing = playing;
            let elapsed = now.duration_since(self.anchor_at).as_millis() as i64;
            let smooth = self.anchor_pos + elapsed;
            let frac = if state.duration_ms > 0 {
                smooth as f32 / state.duration_ms as f32
            } else {
                0.0
            };
            self.progress_slider
                .update(cx, |s, cx| s.set_value(frac.clamp(0.0, 1.0), window, cx));
        }
        // 进度条 Slider 在 Root render 中创建，只有 Root 重绘才会前进：
        // 每次 tick 都重绘（100ms）以获得平滑进度；系统媒体推送节流到秒变化。
        let sec = state.position_ms / 1000;
        self.maybe_continue_roaming(&state, cx);
        self.mirrored = state;
        cx.notify();
        if sec != self.last_sec {
            self.last_sec = sec;
            self.push_media_state(cx);
        }
        while let Ok(command) = self.media_cmds.try_recv() {
            self.apply_media_command(command, cx);
        }
        // 每 5s 落盘一次进度（小文件；队列仅在结构变化时写）
        if self.last_progress_save.elapsed() >= Duration::from_secs(5) {
            self.last_progress_save = Instant::now();
            self.save_progress();
        }
    }

    /// 相似漫游：播放到队列末尾且临近结束时，拉取相似歌曲续接到队列尾部。
    fn maybe_continue_roaming(&mut self, state: &PlaybackState, cx: &mut Context<Self>) {
        if self.player.context() != Some(CONTEXT_ROAMING) {
            return;
        }
        let Some(song) = state.song.as_ref() else {
            return;
        };
        let at_tail = state.queue_len > 0 && state.current_index == Some(state.queue_len - 1);
        if !at_tail || state.duration_ms <= 0 {
            return;
        }
        // 距结束 15s 内触发
        if state.duration_ms - state.position_ms > 15_000 {
            return;
        }
        if self.roaming_last_trigger == song.id {
            return;
        }
        self.roaming_last_trigger = song.id;
        let song_id = song.id;
        let index = state.current_index.unwrap_or(0);
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let songs = siren_core::network::api::simi_songs(&client, song_id)
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });
        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.player.context() != Some(CONTEXT_ROAMING) || songs.is_empty() {
                    return;
                }
                this.player.append_after(index, songs);
                this.save_queue_session();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 落盘队列会话（队列/下标/模式）；仅在结构变化时调用。
    fn save_queue_session(&self) {
        let state = self.player.state();
        let queue = self.player.queue_items();
        if queue.is_empty() {
            return;
        }
        siren_core::playback_session::QueueSession {
            queue,
            index: state.current_index.unwrap_or(0),
            mode: state.mode,
            play_context: self.player.context().map(str::to_string),
            play_source: self.player.source().cloned(),
        }
        .save();
    }

    /// 落盘进度会话（当前曲 id/进度/时长）；高频、文件很小。
    fn save_progress(&self) {
        let state = self.player.state();
        let Some(song) = state.song else {
            return;
        };
        let position_ms = self.resume_ms.unwrap_or(state.position_ms).max(0);
        siren_core::playback_session::ProgressSession {
            song_id: song.id,
            position_ms,
            duration_ms: state.duration_ms.max(0),
        }
        .save();
    }

    /// 同步落盘（退出时调用）：队列 + 进度各写一次。
    pub(crate) fn save_session_now(&self) {
        self.save_queue_session();
        self.save_progress();
    }

    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        // 冷启动恢复后首次播放：先解析 URL 再播（并 seek 回上次进度）
        if self.needs_resolve {
            if let Some(song) = self.player.state().song {
                self.resolve_and_play(song, cx);
                return;
            }
        }
        self.player.toggle();
        self.save_progress();
        cx.notify();
    }

    /// 随机播放：开启随机模式后从给定列表播放。
    pub(crate) fn play_shuffled(&mut self, items: Vec<Song>, cx: &mut Context<Self>) {
        if self.player.mode() != PlayMode::Shuffle {
            self.player.set_mode(PlayMode::Shuffle);
        }
        self.play_from(items, 0, cx);
    }

    /// 用给定歌曲列表建立队列并播放指定下标（无来源容器）。
    pub(crate) fn play_from(&mut self, items: Vec<Song>, index: usize, cx: &mut Context<Self>) {
        self.play_from_source(items, index, None, cx);
    }

    /// 用给定歌曲列表建立队列并播放指定下标，记录来源容器（歌单/专辑）。
    pub(crate) fn play_from_source(
        &mut self,
        items: Vec<Song>,
        index: usize,
        source: Option<PlaySource>,
        cx: &mut Context<Self>,
    ) {
        self.player.set_source(source);
        self.player.set_context(None);
        self.player.clear_snapshot();
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
        self.save_queue_session();
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
        self.save_queue_session();
        cx.notify();
    }

    /// 打卡上报的容器 id：歌单来源用歌单 id，其余用歌曲 id（对齐 Melodia）。
    fn report_container(&self, song_id: i64) -> i64 {
        match self.player.source() {
            Some(source) if source.kind == PlaySourceKind::Playlist => source.id,
            _ => song_id,
        }
    }

    /// 打卡上报上一首的实际播放时长（切歌时）。
    fn finish_previous(&mut self) {
        if let (Some(prev), Some(at)) = (self.playing_id, self.playing_at) {
            let seconds = at.elapsed().as_secs() as i64;
            let container = self.reporting_container;
            let client = self.client.clone();
            self.handle.spawn(async move {
                let _ = siren_core::network::api::report_play_end(&client, prev, container, seconds).await;
            });
        }
    }

    fn resolve_and_play(&mut self, song: Song, cx: &mut Context<Self>) {
        log::info!("解析并播放 id={} name={}", song.id, song.name);
        // 队列/下标已变化：落盘队列会话
        self.save_queue_session();
        // 打卡上报：结束上一首（按实际播放时长），并开始新一首。
        self.finish_previous();
        let client_report = self.client.clone();
        let song_id = song.id;
        let container = self.report_container(song_id);
        // 歌单来源：本地记录「最近播放歌单」（服务端不记录本客户端的歌单播放）
        if let Some(source) = self.player.source() {
            if source.kind == PlaySourceKind::Playlist && source.id != self.last_recorded_playlist {
                self.last_recorded_playlist = source.id;
                siren_core::recent_playlists::record_recent_playlist(
                    siren_core::recent_playlists::LocalRecentPlaylist {
                        id: source.id,
                        name: source.name.clone(),
                        cover_url: source.cover_url.clone(),
                        play_time: siren_core::recent_playlists::now_ms(),
                    },
                );
            }
        }
        self.playing_id = Some(song_id);
        self.playing_at = Some(Instant::now());
        self.reporting_container = container;
        self.handle.spawn(async move {
            let _ = siren_core::network::api::report_start_play(&client_report, song_id, container).await;
        });

        // 拉取歌曲详情（发行时间等），完成后刷新正在播放栏。
        let client_detail = self.client.clone();
        let handle_detail = self.handle.clone();
        self.detail = None;
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_detail.spawn(async move {
                let detail = siren_core::network::api::song_detail(&client_detail, song_id)
                    .await
                    .ok()
                    .flatten();
                let _ = tx.send(detail);
            });
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.detail = detail;
                cx.notify();
            })
            .ok();
        })
        .detach();

        // 拉取歌词（优先逐字），完成后刷新正在播放栏。
        let client_lyric = self.client.clone();
        let handle_lyric = self.handle.clone();
        self.lyrics = Vec::new();
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_lyric.spawn(async move {
                let lines = siren_core::network::api::lyrics(&client_lyric, song_id)
                    .await
                    .unwrap_or_default();
                let _ = tx.send(lines);
            });
            let lines = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.lyrics = lines;
                cx.notify();
            })
            .ok();
        })
        .detach();

        // 拉取歌曲百科/制作人。
        let client_wiki = self.client.clone();
        let handle_wiki = self.handle.clone();
        self.wiki = None;
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_wiki.spawn(async move {
                let wiki = siren_core::network::api::song_wiki(&client_wiki, song_id).await.ok();
                let _ = tx.send(wiki);
            });
            let wiki = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.wiki = wiki;
                cx.notify();
            })
            .ok();
        })
        .detach();

        // 拉取评论 + 首位歌手信息。
        let client_extra = self.client.clone();
        let handle_extra = self.handle.clone();
        let artist_id = song.artist_ids.first().copied();
        self.comments = CommentPage::default();
        self.artist = None;
        self.artist_albums = Vec::new();
        self.similar_artists = Vec::new();
        self.artist_followed = false;
        self.artist_fans = 0;
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_extra.spawn(async move {
                use siren_core::network::api;
                let tid = api::comment_thread_id(song_id);
                let comments = api::comments(&client_extra, &tid, "0", 20).await.unwrap_or_default();
                let artist = match artist_id {
                    Some(aid) => {
                        let detail = api::artist_detail(&client_extra, aid).await.ok().flatten();
                        let albums = api::artist_albums(&client_extra, aid, 10).await.unwrap_or_default();
                        let simi = api::similar_artists(&client_extra, aid).await.unwrap_or_default();
                        let (fans, followed) = api::artist_follow_count(&client_extra, aid).await.unwrap_or((0, false));
                        Some((detail, albums, simi, fans, followed))
                    }
                    None => None,
                };
                let _ = tx.send((comments, artist));
            });
            let (comments, artist) = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.comments = comments;
                let avatars: Vec<String> = this
                    .comments
                    .comments
                    .iter()
                    .map(|c| c.user.avatar_url.clone())
                    .collect();
                this.covers.prefetch(avatars, 32.0, cx);
                if let Some((detail, albums, simi, fans, followed)) = artist {
                    let album_covers: Vec<String> = albums.iter().map(|a| a.cover_url.clone()).collect();
                    let simi_covers: Vec<String> = simi.iter().map(|a| a.cover_url.clone()).collect();
                    let artist_cover: Vec<String> =
                        detail.as_ref().map(|d| vec![d.cover_url.clone()]).unwrap_or_default();
                    this.covers.prefetch(album_covers, 108.0, cx);
                    this.covers.prefetch(simi_covers, 64.0, cx);
                    this.covers.prefetch(artist_cover, 48.0, cx);
                    this.artist = detail;
                    this.artist_albums = albums;
                    this.similar_artists = simi;
                    this.artist_fans = fans;
                    this.artist_followed = followed;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        let client = self.client.clone();
        let id = song.id;
        let level = SettingsStore::of(cx).quality().code();
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
                        log::info!("播放地址解析成功 id={}", song.id);
                        this.player.play_url(&url);
                        this.needs_resolve = false;
                        if let Some(pos) = this.resume_ms.take() {
                            this.player.seek_ms(pos);
                        }
                        this.status = format!("播放中：{}", song.name).into();
                        this.save_progress();
                    }
                    None => {
                        log::warn!("无可用播放地址 id={}", song.id);
                        this.status = "无可用播放地址（需登录/会员）".into();
                        this.notifications
                            .update(cx, |n, cx| n.error("无可用播放地址（需登录/会员）", cx));
                    }
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
        self.save_progress();
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

    fn push_media_state(&self, cx: &App) {
        if !SettingsStore::of(cx).media_keys() {
            return;
        }
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
