//! 播放服务：封装核心 [`Player`] + 网络音源解析 + 系统媒体控制，作为共享 Entity。
//!
//! 界面（`Root` 及各页面）通过 `Entity<Playback>` 订阅/驱动它，而不直接触碰播放细节。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::*;

use super::audio_cache::AudioCache;
use super::covers::CoverLoader;
use super::notifications::Notifications;
use super::settings_store::SettingsStore;
use crate::lyrics_cache::{Cached, LyricsCache};
use siren_core::error::ResultExt;
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
    /// 拖动进度条时的预览比例（0..1）；未拖动为 `None`。
    seek_preview: Option<f32>,
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
    /// 上次推送系统媒体状态的签名（歌曲 id / 播放中 / 模式 / 时长）。
    /// 与整秒节流共同决定何时刷新系统面板（暂停/切模式等不再等到下一整秒）。
    last_media_sig: Option<(Option<i64>, bool, PlayMode, i64)>,
    /// 冷启动恢复的待播放进度（URL 加载后 seek 到此处后清除）。
    resume_ms: Option<i64>,
    /// 冷启动恢复进度归属的歌曲 id：只对该曲生效，用户改播其它曲即丢弃。
    resume_song_id: Option<i64>,
    /// 已恢复但尚未解析 URL（首次播放需先解析）。
    needs_resolve: bool,
    /// URL/元数据解析代次：每次切歌自增，旧回调据此丢弃（防乱序覆盖）。
    resolve_gen: u64,
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
    /// 系统媒体面板的随机 / 单曲循环开关（彼此独立；三态 `PlayMode` 由此派生）。
    sys_shuffle: bool,
    sys_repeat_one: bool,
    /// libmpv 缺失（降级为静默引擎）；首次播放时提示一次。
    mpv_missing: bool,
    mpv_warned: bool,
    progress_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    media: Option<PlatformMediaSession>,
    media_cmds: std::sync::mpsc::Receiver<MediaCommand>,
    /// 播放过的歌曲字节落盘（`cache_dir/audio_cache`，2 GB LRU）；取不到目录时为 None。
    audio_cache: Option<AudioCache>,
    /// 歌词行落盘（`cache_dir/lyrics_cache`，75 MB LRU + 7 天 TTL）。
    lyrics_cache: Option<LyricsCache>,
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
        let (engine, mpv_missing): (Arc<dyn PlaybackController>, bool) =
            match Mpv::load(&siren_platform::mpv_library_candidates()) {
                Ok(mpv) => (Arc::new(mpv), false),
                Err(e) => {
                    log::warn!("未找到 libmpv，播放不可用：{e}");
                    (Arc::new(SilentEngine), true)
                }
            };
        let mut player = Player::new(engine);
        // 冷启动恢复上次队列/当前曲/进度（不自动播放；首次播放时解析 URL 并 seek 回进度）
        let mut resume_ms: Option<i64> = None;
        let mut resume_song_id: Option<i64> = None;
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
                resume_song_id = current_id;
                needs_resolve = true;
            }
        }

        // 由恢复出的播放模式回填随机/单曲循环开关（三态派生）。
        let sys_shuffle = player.mode() == PlayMode::Shuffle;
        let sys_repeat_one = player.mode() == PlayMode::SingleLoop;

        // 系统媒体控制：取原生句柄交给 playwire 的 SMTC 配置
        //（低层接口——playwire 的配置是 u64；常规窗口操作用 siren_platform 的便捷函数，直接传窗口）。
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
                    SliderEvent::Change(SliderValue::Single(f)) => {
                        // 拖动中只预览（不 seek，避免连续 seek 噪音）；左侧时间标签同步跟随。
                        this.seeking = true;
                        this.seek_preview = Some(*f);
                        cx.notify();
                    }
                    SliderEvent::Release(SliderValue::Single(f)) => {
                        this.seek_fraction(*f, cx);
                        this.seeking = false;
                        this.seek_preview = None;
                        cx.notify();
                    }
                    _ => {}
                },
            );
            // 音量初值取自设置（默认 80），并立即对齐到引擎，避免「滑条 80% 而引擎 100%」的跳变。
            let initial_volume = SettingsStore::of(cx).volume();
            player.set_volume(initial_volume);
            let volume_slider = cx.new(|_| {
                SliderState::new()
                    .min(0.0)
                    .max(1.0)
                    .step(0.01)
                    .default_value((initial_volume / 100.0) as f32)
            });
            let _volume_sub = cx.subscribe(
                &volume_slider,
                |this: &mut Playback, _slider, event: &SliderEvent, cx| {
                    match event {
                        SliderEvent::Change(SliderValue::Single(f)) => {
                            this.player.set_volume(*f as f64 * 100.0);
                            cx.notify();
                        }
                        // 拖动结束才落盘，避免逐像素写盘。
                        SliderEvent::Release(SliderValue::Single(f)) => {
                            cx.global_mut::<SettingsStore>().set_volume(*f as f64 * 100.0);
                        }
                        _ => {}
                    }
                },
            );
            // 音频缓存：取不到系统缓存目录时降级为「不落盘」，播放不受影响。
            let audio_cache = AudioCache::open(&handle);
            let lyrics_cache = LyricsCache::open(&handle);
            Self {
                client,
                handle,
                covers,
                audio_cache,
                lyrics_cache,
                notifications,
                player,
                status: "就绪".into(),
                mirrored: PlaybackState::default(),
                seeking: false,
                seek_preview: None,
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
                last_media_sig: None,
                resume_ms,
                resume_song_id,
                needs_resolve,
                resolve_gen: 0,
                last_progress_save: Instant::now(),
                anchor_pos: 0,
                anchor_at: Instant::now(),
                was_playing: false,
                playing_id: None,
                playing_at: None,
                reporting_container: 0,
                last_recorded_playlist: 0,
                roaming_last_trigger: 0,
                sys_shuffle,
                sys_repeat_one,
                mpv_missing,
                mpv_warned: false,
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
                    .ok_or_warn("加载歌单详情")
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
            let _ = tx.send(
                siren_core::network::api::album_detail(&client, id)
                    .await
                    .ok_or_warn("加载专辑详情")
                    .flatten(),
            );
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
            let _ = tx.send(
                siren_core::network::api::song_detail(&client, id)
                    .await
                    .ok_or_warn("加载歌曲详情")
                    .flatten(),
            );
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
                    None => log::warn!("喜欢歌曲：任务未返回（通道断开）"),
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

    /// 音频缓存是否开启（设置项，默认开）。
    pub(crate) fn audio_cache_enabled(&self, cx: &App) -> bool {
        SettingsStore::of(cx).audio_cache()
    }

    /// 开关音频缓存并落盘。
    pub(crate) fn set_audio_cache_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        cx.global_mut::<SettingsStore>().set_audio_cache(enabled);
        cx.notify();
    }

    /// 音频缓存用量（原子量快照，渲染路径可安全调用）。
    pub(crate) fn audio_cache_stats(&self) -> crate::disk_lru::DiskStats {
        self.audio_cache.as_ref().map(|c| c.stats()).unwrap_or_default()
    }

    /// 音频缓存上限（字节）；无缓存目录时为 0。
    pub(crate) fn audio_cache_limit(&self) -> u64 {
        self.audio_cache.as_ref().map(|c| c.limit()).unwrap_or(0)
    }

    /// 歌词缓存用量（原子量快照）。
    pub(crate) fn lyrics_cache_stats(&self) -> crate::disk_lru::DiskStats {
        self.lyrics_cache.as_ref().map(|c| c.stats()).unwrap_or_default()
    }

    /// 歌词缓存上限（字节）；无缓存目录时为 0。
    pub(crate) fn lyrics_cache_limit(&self) -> u64 {
        self.lyrics_cache.as_ref().map(|c| c.limit()).unwrap_or(0)
    }

    /// 清空歌词缓存（下次播放重新请求）。
    pub(crate) fn clear_lyrics_cache(&mut self, cx: &mut Context<Self>) {
        if let Some(cache) = self.lyrics_cache.clone() {
            self.handle.spawn(async move {
                let removed = tokio::task::spawn_blocking(move || cache.clear()).await.unwrap_or(0);
                log::info!("已清除歌词缓存：{removed} 条");
            });
        }
        cx.notify();
    }

    /// 清空音频缓存（下次播放重新下载）。
    pub(crate) fn clear_audio_cache(&mut self, cx: &mut Context<Self>) {
        if let Some(cache) = self.audio_cache.clone() {
            self.handle.spawn(async move {
                let removed = tokio::task::spawn_blocking(move || cache.clear()).await.unwrap_or(0);
                log::info!("已清除音频缓存：{removed} 个文件");
            });
        }
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

    /// 切换对当前歌手的关注（乐观更新 + 失败回滚，对齐 `toggle_like_song`）。
    pub(crate) fn toggle_artist_follow(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.artist.as_ref().map(|a| a.id) else {
            return;
        };
        let follow = !self.artist_followed;
        self.artist_followed = follow;
        cx.notify();
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::artist_follow(&client, id, follow).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(())) => {}
                    Some(Err(err)) => {
                        // 失败回滚开关并提示，避免「看似已关注实则未生效」。
                        this.artist_followed = !follow;
                        this.notifications.update(cx, |n, cx| n.error(err.user_message(), cx));
                    }
                    // 通道断开（任务取消等）：保持现状不重试，仅回滚乐观态。
                    None => {
                        this.artist_followed = !follow;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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

    /// 拖动进度条时的预览位置（毫秒）；未拖动返回 `None`。
    pub(crate) fn seek_preview_ms(&self) -> Option<i64> {
        let f = self.seek_preview?;
        let dur = self.player.state().duration_ms.max(0);
        Some((f.clamp(0.0, 1.0) as f64 * dur as f64) as i64)
    }

    pub(crate) fn volume_slider(&self) -> &Entity<SliderState> {
        &self.volume_slider
    }

    fn start_tick_timer(&mut self, window: &Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            // 100ms：进度条约 10fps 足够平滑；曾经的 16ms/60fps 空闲也全量重绘。
            cx.background_executor().timer(Duration::from_millis(100)).await;
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
        // 播放中按 100ms 节拍重绘以获得平滑进度；空闲（暂停且无变化）不再重绘。
        let sec = state.position_ms / 1000;
        // 系统媒体状态签名：歌曲 / 播放态 / 模式 / 时长（不含进度，进度由整秒节流驱动）。
        let media_sig = (
            state.song.as_ref().map(|s| s.id),
            state.playing,
            state.mode,
            state.duration_ms,
        );
        self.maybe_continue_roaming(&state, cx);
        let changed = state != self.mirrored;
        let playing = state.playing;
        self.mirrored = state;
        if changed || playing {
            cx.notify();
        }
        // 整秒边界刷新进度；播放态/模式/歌曲/时长变化则立即刷新——否则暂停后 position 冻结、
        // sec 不再变化，系统媒体面板会一直停留在「播放中」（B）。
        let media_changed = self.last_media_sig != Some(media_sig);
        self.last_media_sig = Some(media_sig);
        if sec != self.last_sec || media_changed {
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
                // 回调期间用户可能已切歌：重读当前下标并校验仍指向触发续接的那首歌，
                // 否则用陈旧 index 截断会砍掉队列尾部 / 丢失当前曲（J5）。
                let cur = this.player.state();
                if cur.song.as_ref().map(|s| s.id) != Some(song_id) {
                    return;
                }
                let Some(index) = cur.current_index else {
                    return;
                };
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
            // 清空队列后删除旧会话，否则重启会把旧队列“复活”。
            siren_core::playback_session::QueueSession::clear();
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
        // 同步系统媒体面板的随机开关状态。
        self.sys_shuffle = true;
        self.sys_repeat_one = false;
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

    /// 播放队列快照与当前下标（供队列面板渲染）。
    pub(crate) fn queue_snapshot(&self) -> (Vec<Song>, Option<usize>) {
        (self.player.queue_items(), self.player.current_index())
    }

    /// 当前播放歌曲 id（无当前曲返回 `None`；供页面高亮当前行）。
    pub(crate) fn current_song_id(&self) -> Option<i64> {
        self.player.state().song.map(|s| s.id)
    }

    /// 播放队列中指定下标的曲目。
    pub(crate) fn play_queue_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(song) = self.player.play_at(index) {
            self.resolve_and_play(song, cx);
        }
    }

    /// 从队列移除指定下标；若移除的是当前曲，则重载新的当前曲。
    pub(crate) fn remove_queue_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let before = self.player.state().song.map(|s| s.id);
        self.player.remove_at(index);
        // 队列结构已变：落盘，避免重启后“复活”被删的曲目。
        self.save_queue_session();
        let after = self.player.state().song.map(|s| s.id);
        if before != after {
            if let Some(song) = self.player.state().song {
                self.resolve_and_play(song, cx);
            }
        } else {
            cx.notify();
        }
    }

    /// 队列内拖动排序。
    pub(crate) fn move_in_queue(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.player.move_in_queue(from, to);
        self.save_queue_session();
        cx.notify();
    }

    /// 清空队列（保留当前曲）。
    pub(crate) fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.player.clear_queue();
        self.save_queue_session();
        cx.notify();
    }

    /// 切换随机播放（与单曲循环相互独立）。
    pub(crate) fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.sys_shuffle = !self.sys_shuffle;
        self.apply_sys_mode(cx);
    }

    /// 切换单曲循环（与随机相互独立）。
    pub(crate) fn toggle_repeat(&mut self, cx: &mut Context<Self>) {
        self.sys_repeat_one = !self.sys_repeat_one;
        self.apply_sys_mode(cx);
    }

    /// 由随机/单曲循环开关派生三态 `PlayMode`。随机优先（无「随机+单曲循环」组合）。
    fn apply_sys_mode(&mut self, cx: &mut Context<Self>) {
        let mode = if self.sys_shuffle {
            PlayMode::Shuffle
        } else if self.sys_repeat_one {
            PlayMode::SingleLoop
        } else {
            PlayMode::ListLoop
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

    /// 拉取当前曲的详情 / 歌词 / 百科 / 评论 / 首位歌手；`gen` 用于丢弃切歌后的乱序结果。
    fn fetch_song_metadata(&mut self, song: &Song, gen: u64, cx: &mut Context<Self>) {
        let song_id = song.id;
        // 拉取歌曲详情（发行时间等），完成后刷新正在播放栏。
        let client_detail = self.client.clone();
        let handle_detail = self.handle.clone();
        self.detail = None;
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_detail.spawn(async move {
                let detail = siren_core::network::api::song_detail(&client_detail, song_id)
                    .await
                    .ok_or_warn("加载歌曲详情")
                    .flatten();
                let _ = tx.send(detail);
            });
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if this.resolve_gen != gen {
                    return;
                }
                this.detail = detail;
                cx.notify();
            })
            .ok();
        })
        .detach();

        // 拉取歌词（优先逐字），完成后刷新正在播放栏。
        let client_lyric = self.client.clone();
        let handle_lyric = self.handle.clone();
        let cache_lyric = self.lyrics_cache.clone();
        self.lyrics = Vec::new();
        cx.spawn(async move |this, cx| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle_lyric.spawn(async move {
                // ① 磁盘命中（7 天 TTL；空结果 12h 负缓存）：不再请求接口。
                if let Some(cache) = cache_lyric.as_ref() {
                    match cache.load(song_id) {
                        Some(Cached::Lines(lines)) => {
                            log::debug!("歌词来源：本地缓存 id={song_id}（{} 行）", lines.len());
                            let _ = tx.send(lines);
                            return;
                        }
                        Some(Cached::Empty) => {
                            log::debug!("歌词来源：负缓存 id={song_id}（12h 内确认无歌词）");
                            let _ = tx.send(Vec::new());
                            return;
                        }
                        None => {}
                    }
                }
                // ② 未命中：请求接口并回写（空结果也写，供负缓存）。
                let lines = siren_core::network::api::lyrics(&client_lyric, song_id)
                    .await
                    .unwrap_or_default();
                log::debug!("歌词来源：网络 id={song_id}（{} 行）", lines.len());
                if let Some(cache) = cache_lyric.as_ref() {
                    cache.save(song_id, &lines);
                }
                let _ = tx.send(lines);
            });
            let lines = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.resolve_gen != gen {
                    return;
                }
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
                let wiki = siren_core::network::api::song_wiki(&client_wiki, song_id)
                    .await
                    .ok_or_warn("加载歌曲百科");
                let _ = tx.send(wiki);
            });
            let wiki = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if this.resolve_gen != gen {
                    return;
                }
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
                        let detail = api::artist_detail(&client_extra, aid)
                            .await
                            .ok_or_warn("加载歌手详情")
                            .flatten();
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
                if this.resolve_gen != gen {
                    return;
                }
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
    }

    /// 冷启动恢复出的当前曲：立即拉取元数据（详情/歌词/百科/评论/歌手），无需等用户按播放。
    pub(crate) fn load_restored_metadata(&mut self, cx: &mut Context<Self>) {
        if !self.needs_resolve {
            return;
        }
        let Some(song) = self.player.state().song else {
            return;
        };
        self.resolve_gen = self.resolve_gen.wrapping_add(1);
        let gen = self.resolve_gen;
        self.fetch_song_metadata(&song, gen, cx);
    }

    fn resolve_and_play(&mut self, song: Song, cx: &mut Context<Self>) {
        log::info!("解析并播放 id={} name={}", song.id, song.name);
        // mpv 缺失时整条链路「看似正常但无声」：首次播放提示一次。
        if self.mpv_missing && !self.mpv_warned {
            self.mpv_warned = true;
            self.notifications
                .update(cx, |n, cx| n.error("未找到 mpv，播放不可用", cx));
        }
        // 代次自增：本次解析的所有异步回调都带 gen，切歌后旧回调据此丢弃。
        self.resolve_gen = self.resolve_gen.wrapping_add(1);
        let gen = self.resolve_gen;
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

        // 详情/歌词/百科/评论/歌手：与播放解耦，冷启动恢复时也会调用（见 `load_restored_metadata`）。
        self.fetch_song_metadata(&song, gen, cx);

        let level = SettingsStore::of(cx).quality().code();
        let cache_on = SettingsStore::of(cx).audio_cache();
        let cached = if cache_on {
            self.audio_cache.as_ref().and_then(|c| c.lookup(song_id, level))
        } else {
            None
        };
        // 「这一首从哪来」一条 INFO 日志说清：本地缓存 / 网络（含为什么没命中）。
        match &cached {
            Some(path) => log::info!(
                "音频来源：本地缓存 id={song_id} level={level}「{}」文件={}",
                song.name,
                path.display()
            ),
            None => log::info!(
                "音频来源：网络 id={song_id} level={level}「{}」（{}）",
                song.name,
                if !cache_on {
                    "音频缓存已关闭"
                } else if self.audio_cache.is_none() {
                    "本地缓存不可用"
                } else {
                    "缓存未命中，播放后写入"
                }
            ),
        }
        // 命中：直接播本地文件——连播放地址都不用解析，断网也能放。
        if let Some(path) = cached {
            let resume = self.resume_ms.take().filter(|_| Some(song.id) == self.resume_song_id);
            self.resume_song_id = None;
            self.player.play_url_from(path.to_string_lossy().as_ref(), resume);
            self.needs_resolve = false;
            self.status = format!("播放中：{}", song.name).into();
            cx.notify();
            return;
        }

        let client = self.client.clone();
        let id = song.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.handle.spawn(async move {
            let url = match siren_core::network::api::song_url_eapi(&client, id, level)
                .await
                .ok_or_warn("解析播放地址")
                .flatten()
            {
                Some(url) => Some(url),
                None => siren_core::network::api::song_url(&client, id)
                    .await
                    .ok_or_warn("解析备选播放地址")
                    .flatten(),
            };
            let _ = tx.send(url);
        });

        self.status = format!("解析播放地址：{}", song.name).into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let url = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                // 代次不符：用户已切到别的歌，丢弃这次（可能乱序到达的）解析结果。
                if this.resolve_gen != gen {
                    return;
                }
                // 冷启动进度只归属恢复出的那首歌；用户改播其它曲则丢弃，避免强锁到旧进度。
                let resume = this.resume_ms.take().filter(|_| Some(song.id) == this.resume_song_id);
                this.resume_song_id = None;
                match url {
                    Some(url) => {
                        log::info!("播放地址解析成功 id={}", song.id);
                        // 冷启动恢复：让引擎加载完成后自动跳到上次进度，避免加载前 seek 被丢弃。
                        this.player.play_url_from(&url, resume);
                        this.needs_resolve = false;
                        this.status = format!("播放中：{}", song.name).into();
                        this.save_progress();
                        // 后台把这次的音频写入缓存（不阻塞起播）；播放地址有时效，尽早拉。
                        if cache_on {
                            if let Some(cache) = this.audio_cache.clone() {
                                let client = this.client.clone();
                                let cache_url = url.clone();
                                let cache_level = level;
                                let cache_id = song_id;
                                this.handle.spawn(async move {
                                    match client.get_bytes(&cache_url).await {
                                        Ok(bytes) => {
                                            if cache.store(cache_id, cache_level, &bytes) {
                                                log::info!(
                                                    "音频缓存写入完成 id={cache_id} level={cache_level} {} KB",
                                                    bytes.len() / 1024
                                                );
                                            }
                                        }
                                        Err(err) => log::warn!("音频缓存下载失败 id={cache_id}：{err}"),
                                    }
                                });
                            }
                        }
                    }
                    None => {
                        log::warn!("无可用播放地址 id={}", song.id);
                        // 解除「等待加载」闩锁，避免状态机卡死不再推进。
                        this.player.cancel_pending_load();
                        this.needs_resolve = false;
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
        // 冷启动恢复曲尚未加载 URL：把拖动结果记为起播进度，按压播放时据此起播，否则会被旧进度覆盖。
        if self.needs_resolve {
            self.resume_ms = Some(ms);
        }
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
            // 与播放栏按钮同源：更新独立开关后派生模式。
            // 关随机不再被吞（会回落到单曲循环或顺序）；开/关重复也不会抹掉随机。
            MediaCommand::SetShuffle(on) => {
                self.sys_shuffle = on;
                self.apply_sys_mode(cx);
            }
            MediaCommand::SetRepeat(one) => {
                self.sys_repeat_one = one;
                self.apply_sys_mode(cx);
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
