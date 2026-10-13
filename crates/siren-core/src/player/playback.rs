//! 播放器：队列 + 引擎控制 + 状态广播。
//!
//! ## 单向数据流
//!
//! ```text
//! 命令（play_at/next/toggle/...） → Player → 引擎
//!                                      └→ PlaybackState → watch 订阅者（UI / 平台层）
//! ```
//!
//! 订阅者通过 [`Player::subscribe`] 拿到 `watch::Receiver<PlaybackState>`，
//! 用 `.borrow()` 取当前状态、`.changed().await` 等待变化。
//!
//! ## 加载入口
//!
//! `Player` 不负责解析播放地址（那是异步网络层的职责）。需要加载时，
//! 各命令与 [`Player::tick`] 会返回**待加载的歌曲**，由调用方解析出 URL 后
//! 调用 [`Player::play_url`] 完成加载。
//!
//! ## 驱动
//!
//! 调用方按固定节奏调用 [`Player::tick`]（如每 500ms）刷新进度、同步暂停态，
//! 并在自然播完后按播放模式自动切歌。

use std::sync::Arc;

use tokio::sync::watch;

use crate::model::Song;
use crate::player::{PlayMode, PlaybackController, PlayerQueue};

/// 播放状态快照（可比较，便于变更检测）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaybackState {
    pub song: Option<Song>,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub mode: PlayMode,
    pub queue_len: usize,
    pub current_index: Option<usize>,
}

/// 播放器：持有音频引擎与队列，广播状态。
pub struct Player {
    engine: Arc<dyn PlaybackController>,
    queue: PlayerQueue,
    state: PlaybackState,
    tx: watch::Sender<PlaybackState>,
    /// EOF 闩锁：mpv 在新文件加载前会持续报告 eof，避免重复推进队列。
    eof_latched: bool,
    /// 等待加载闩锁：已切歌但新 URL 尚未经 [`Player::play_url`] 送达引擎。
    /// 此窗口内引擎仍在报告**上一首**的 `time-pos`/`duration`，必须忽略，
    /// 否则会把刚重置的进度又覆盖回旧曲尾部（表现为进度条尾部来回跳动）。
    awaiting_load: bool,
    /// 上一次观察到的 eof 状态（仅用于日志）。
    last_eof: bool,
}

impl Player {
    pub fn new(engine: Arc<dyn PlaybackController>) -> Self {
        let state = PlaybackState::default();
        let (tx, _rx) = watch::channel(state.clone());
        Self {
            engine,
            queue: PlayerQueue::new(),
            state,
            tx,
            eof_latched: false,
            awaiting_load: false,
            last_eof: false,
        }
    }

    /// 订阅状态流。
    pub fn subscribe(&self) -> watch::Receiver<PlaybackState> {
        self.tx.subscribe()
    }

    /// 当前状态快照。
    pub fn state(&self) -> PlaybackState {
        self.state.clone()
    }

    /// 当前歌曲线程安全的引擎句柄。
    pub fn engine(&self) -> Arc<dyn PlaybackController> {
        self.engine.clone()
    }

    /// 替换队列并选中首曲；返回需要加载的歌曲。
    pub fn set_queue(&mut self, items: Vec<Song>) -> Option<Song> {
        self.queue.set_items(items);
        self.after_selection()
    }

    /// 选中指定下标；返回需要加载的歌曲（越界时返回 `None`）。
    pub fn play_at(&mut self, index: usize) -> Option<Song> {
        if self.queue.select(index) {
            self.after_selection()
        } else {
            None
        }
    }

    /// 下一首（手动，顺序回绕）；返回需要加载的歌曲。
    /// 当前队列快照（用于持久化）。
    pub fn queue_items(&self) -> Vec<Song> {
        self.queue.items().to_vec()
    }

    /// 队列来源容器。
    pub fn source(&self) -> Option<&crate::player::PlaySource> {
        self.queue.source()
    }

    /// 设置队列来源容器。
    pub fn set_source(&mut self, source: Option<crate::player::PlaySource>) {
        self.queue.set_source(source);
    }

    /// 播放上下文标签。
    pub fn context(&self) -> Option<&str> {
        self.queue.context()
    }

    /// 把 `song` 插到当前曲之后（「下一首播放」）。
    pub fn insert_next(&mut self, song: Song) {
        self.queue.insert_next(song);
        self.sync_queue();
        self.publish();
    }

    /// 用 `items` 替换 `index` 之后的曲目（漫游续接）。
    pub fn append_after(&mut self, index: usize, items: Vec<Song>) {
        self.queue.append_after(index, items);
        self.sync_queue();
        self.publish();
    }

    /// 设置播放上下文标签。
    pub fn set_context(&mut self, context: Option<String>) {
        self.queue.set_context(context);
    }

    /// 备份当前队列（进入心动/漫游前）。
    pub fn take_snapshot(&mut self) {
        self.queue.take_snapshot();
    }

    /// 丢弃队列备份（正常播放新队列时）。
    pub fn clear_snapshot(&mut self) {
        self.queue.clear_snapshot();
    }

    /// 是否处于可退出的特殊上下文（有备份或带上下文标签）。
    pub fn in_special_context(&self) -> bool {
        self.queue.context().is_some()
    }

    /// 退出特殊上下文：还原备份队列（若有），返回退出前的当前曲 id。
    pub fn exit_special_context(&mut self) -> Option<i64> {
        let playing = self.state.song.as_ref().map(|s| s.id);
        self.queue.exit_special_context();
        self.sync_queue();
        self.publish();
        playing
    }

    /// 恢复队列/当前曲/进度（不加载音频；等待调用方解析 URL 后 [`Player::play_url`]）。
    ///
    /// 恢复后进入「等待加载」窗口：引擎尚未加载新文件，期间跳过进度同步。
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        &mut self,
        queue: Vec<Song>,
        index: usize,
        mode: PlayMode,
        position_ms: i64,
        duration_ms: i64,
        source: Option<crate::player::PlaySource>,
        context: Option<String>,
    ) -> Option<Song> {
        if queue.is_empty() {
            return None;
        }
        self.queue.set_items(queue);
        self.queue.set_mode(mode);
        self.queue.set_source(source);
        self.queue.set_context(context);
        let idx = index.min(self.queue.len().saturating_sub(1));
        self.queue.select(idx);
        self.sync_queue();
        self.state.position_ms = position_ms.max(0);
        self.state.duration_ms = duration_ms.max(0);
        self.state.playing = false;
        self.awaiting_load = true;
        self.eof_latched = false;
        self.publish();
        self.state.song.clone()
    }

    pub fn next_track(&mut self) -> Option<Song> {
        let index = self.queue.next_index()?;
        self.queue.select(index);
        self.after_selection()
    }

    /// 上一首（手动，顺序回绕）；返回需要加载的歌曲。
    pub fn previous_track(&mut self) -> Option<Song> {
        let index = self.queue.previous_index()?;
        self.queue.select(index);
        self.after_selection()
    }

    /// 引擎加载并开始播放指定地址。
    pub fn play_url(&mut self, url: &str) {
        log::info!(
            "加载播放地址 song={:?} len={}",
            self.state.song.as_ref().map(|s| s.id),
            url.len()
        );
        self.engine.load(url);
        self.engine.set_paused(false);
        log::debug!("引擎已加载新文件，恢复进度同步");
        // 新文件已交给引擎，之后读到的是新曲进度，可恢复位置/时长同步。
        self.awaiting_load = false;
        self.state.playing = true;
        self.publish();
    }

    /// 切换播放/暂停（需已有当前曲）。
    pub fn toggle(&mut self) {
        if self.state.song.is_none() {
            return;
        }
        let playing = !self.state.playing;
        self.engine.set_paused(!playing);
        self.state.playing = playing;
        self.publish();
    }

    pub fn play(&mut self) {
        if self.state.song.is_some() {
            self.engine.set_paused(false);
            self.state.playing = true;
            self.publish();
        }
    }

    pub fn pause(&mut self) {
        if self.state.song.is_some() {
            self.engine.set_paused(true);
            self.state.playing = false;
            self.publish();
        }
    }

    /// 跳转到绝对位置（毫秒）。
    pub fn seek_ms(&mut self, position_ms: i64) {
        let ms = position_ms.max(0);
        self.engine.seek_to_ms(ms);
        self.state.position_ms = ms;
        self.publish();
    }

    pub fn set_volume(&self, percent: f64) {
        self.engine.set_volume(percent);
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.queue.set_mode(mode);
        self.state.mode = mode;
        self.publish();
    }

    pub fn mode(&self) -> PlayMode {
        self.queue.mode()
    }

    /// 刷新进度/暂停态；自然播完时按模式切歌。
    ///
    /// 返回因自动切歌而**需要加载**的歌曲（调用方解析 URL 后调用 [`Player::play_url`]）。
    pub fn tick(&mut self) -> Option<Song> {
        let mut dirty = false;

        // 切歌后、新文件真正加载前（awaiting_load），引擎仍在报告上一首的
        // time-pos/duration/pause，此时读取会把刚重置的进度覆盖回旧曲尾部，
        // 表现为进度条在尾部来回跳动。该窗口内一律跳过引擎进度同步。
        if !self.awaiting_load {
            if let Some(pos) = self.engine.position_ms() {
                if (pos - self.state.position_ms).abs() >= 250 {
                    self.state.position_ms = pos;
                    dirty = true;
                }
            }
            if let Some(dur) = self.engine.duration_ms() {
                if dur != self.state.duration_ms {
                    self.state.duration_ms = dur;
                    dirty = true;
                }
            }
            if let Some(paused) = self.engine.is_paused() {
                let playing = !paused && self.state.song.is_some();
                if playing != self.state.playing {
                    self.state.playing = playing;
                    dirty = true;
                }
            }
        }

        let mut pending = None;
        let eof = self.engine.eof_reached().unwrap_or(false);
        if eof != self.last_eof {
            log::debug!(
                "eof={eof} pos={} dur={} playing={} song={:?} latched={}",
                self.state.position_ms,
                self.state.duration_ms,
                self.state.playing,
                self.state.song.as_ref().map(|s| s.id),
                self.eof_latched,
            );
            self.last_eof = eof;
        }
        if !eof {
            // mpv 已开始播放新文件，解除闩锁
            self.eof_latched = false;
        }
        if self.state.song.is_some() && eof && !self.eof_latched {
            // 只推进一次；在新文件真正播放（eof 变假）前不再重复推进
            self.eof_latched = true;
            match self.advance_auto() {
                Some(song) => {
                    log::info!("自然播完，自动切歌 -> song={}", song.id);
                    pending = Some(song);
                }
                None => log::warn!("自然播完但队列已到末尾，无下一首"),
            }
            dirty = true;
        }

        if dirty {
            self.publish();
        }
        pending
    }

    /// 选中变化后统一更新状态；返回需要加载的歌曲。
    fn after_selection(&mut self) -> Option<Song> {
        self.sync_queue();
        self.state.position_ms = 0;
        self.state.duration_ms = 0;
        self.state.playing = false;
        // 已选中新曲但 URL 尚未加载：进入等待加载窗口，暂不同步引擎进度。
        self.awaiting_load = true;
        self.publish();
        self.state.song.clone()
    }

    /// 自动切歌（遵循播放模式），返回需要加载的歌曲。
    fn advance_auto(&mut self) -> Option<Song> {
        self.queue.advance()?;
        self.sync_queue();
        self.state.position_ms = 0;
        self.state.duration_ms = 0;
        self.state.playing = false;
        // 自然播完的切歌同样要等新 URL 送达引擎，期间忽略旧曲残留进度。
        self.awaiting_load = true;
        self.state.song.clone()
    }

    fn sync_queue(&mut self) {
        self.state.song = self.queue.current().cloned();
        self.state.current_index = self.queue.current_index();
        self.state.queue_len = self.queue.len();
    }

    fn publish(&self) {
        self.tx.send_replace(self.state.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockInner {
        url: Option<String>,
        paused: bool,
        pos_ms: i64,
        dur_ms: i64,
        eof: bool,
    }

    /// 无 libmpv 的假引擎，便于测试状态机。
    struct MockEngine(Mutex<MockInner>);

    impl MockEngine {
        fn new() -> Self {
            Self(Mutex::new(MockInner::default()))
        }
        fn set(&self, f: impl FnOnce(&mut MockInner)) {
            f(&mut self.0.lock().unwrap());
        }
        fn url(&self) -> Option<String> {
            self.0.lock().unwrap().url.clone()
        }
    }

    impl PlaybackController for MockEngine {
        fn load(&self, url: &str) {
            self.set(|i| {
                i.url = Some(url.to_string());
                i.eof = false;
            });
        }
        fn set_paused(&self, paused: bool) {
            self.set(|i| i.paused = paused);
        }
        fn stop(&self) {}
        fn seek_to_ms(&self, position_ms: i64) {
            self.set(|i| i.pos_ms = position_ms);
        }
        fn set_volume(&self, _percent: f64) {}
        fn position_ms(&self) -> Option<i64> {
            Some(self.0.lock().unwrap().pos_ms)
        }
        fn duration_ms(&self) -> Option<i64> {
            Some(self.0.lock().unwrap().dur_ms)
        }
        fn is_paused(&self) -> Option<bool> {
            Some(self.0.lock().unwrap().paused)
        }
        fn eof_reached(&self) -> Option<bool> {
            Some(self.0.lock().unwrap().eof)
        }
    }

    fn songs(n: i64) -> Vec<Song> {
        (0..n)
            .map(|id| Song {
                id,
                name: format!("song{id}"),
                artists: String::new(),
                album: String::new(),
                cover_url: String::new(),
                duration_ms: 0,
                publish_time: 0,
                artist_ids: Vec::new(),
            })
            .collect()
    }

    fn setup(n: i64) -> (Player, Arc<MockEngine>) {
        let engine = Arc::new(MockEngine::new());
        let mut player = Player::new(engine.clone());
        player.set_queue(songs(n));
        (player, engine)
    }

    #[test]
    fn set_queue_selects_first_and_publishes() {
        let (player, _) = setup(3);
        let state = player.state();
        assert_eq!(state.current_index, Some(0));
        assert_eq!(state.song.unwrap().id, 0);
        assert_eq!(state.queue_len, 3);
    }

    #[test]
    fn subscriber_receives_updates() {
        let (mut player, _) = setup(3);
        let rx = player.subscribe();
        assert_eq!(rx.borrow().current_index, Some(0));
        let _ = player.next_track();
        assert_eq!(rx.borrow().current_index, Some(1));
    }

    #[test]
    fn play_url_marks_playing_and_loads_engine() {
        let (mut player, engine) = setup(1);
        player.play_url("http://example/song.mp3");
        assert!(player.state().playing);
        assert_eq!(engine.url().as_deref(), Some("http://example/song.mp3"));
    }

    #[test]
    fn toggle_flips_playing() {
        let (mut player, _) = setup(1);
        player.play_url("u");
        player.toggle();
        assert!(!player.state().playing);
        player.toggle();
        assert!(player.state().playing);
    }

    #[test]
    fn next_and_previous_wrap() {
        let (mut player, _) = setup(3);
        assert_eq!(player.next_track().unwrap().id, 1);
        assert_eq!(player.next_track().unwrap().id, 2);
        assert_eq!(player.next_track().unwrap().id, 0);
        assert_eq!(player.previous_track().unwrap().id, 2);
    }

    #[test]
    fn seek_updates_position() {
        let (mut player, engine) = setup(1);
        player.seek_ms(12_000);
        assert_eq!(player.state().position_ms, 12_000);
        assert_eq!(engine.0.lock().unwrap().pos_ms, 12_000);
    }

    #[test]
    fn tick_reflects_engine_progress() {
        let (mut player, engine) = setup(1);
        player.play_url("u");
        engine.set(|i| {
            i.pos_ms = 5_000;
            i.dur_ms = 100_000;
        });
        assert!(player.tick().is_none());
        let state = player.state();
        assert_eq!(state.position_ms, 5_000);
        assert_eq!(state.duration_ms, 100_000);
    }

    #[test]
    fn eof_auto_advances_list_loop() {
        let (mut player, engine) = setup(3);
        player.play_url("u0");
        engine.set(|i| i.eof = true);
        let pending = player.tick().expect("应返回待加载歌曲");
        assert_eq!(pending.id, 1);
        assert_eq!(player.state().current_index, Some(1));
        assert!(!player.state().playing, "切歌后等待调用方加载");
    }

    #[test]
    fn eof_single_loop_keeps_same_song() {
        let (mut player, engine) = setup(3);
        player.set_mode(PlayMode::SingleLoop);
        player.play_at(1);
        engine.set(|i| i.eof = true);
        let pending = player.tick().unwrap();
        assert_eq!(pending.id, 1);
    }

    /// 回归：切歌后引擎仍报告上一首尾部进度，不得把刚重置的进度覆盖回尾部。
    /// 复现「进度条到尾部来回跳动、无法进入下一首」。
    #[test]
    fn advance_ignores_stale_engine_tail_until_new_url() {
        let (mut player, engine) = setup(3);
        player.play_url("u0");
        // 上一首播到尾：引擎 position/duration 都在尾部，eof 为真。
        engine.set(|i| {
            i.eof = true;
            i.pos_ms = 200_000;
            i.dur_ms = 200_000;
        });

        // 自动切歌：应返回下一首，并重置进度。
        let pending = player.tick().expect("应返回待加载歌曲");
        assert_eq!(pending.id, 1);
        assert_eq!(player.state().position_ms, 0);
        assert_eq!(player.state().duration_ms, 0);

        // 关键：新 URL 尚未送达（awaiting_load），引擎仍报旧曲尾部；
        // 后续 tick 不得把进度覆盖回 200_000。
        engine.set(|i| {
            i.pos_ms = 200_000;
            i.dur_ms = 200_000;
        });
        for _ in 0..10 {
            assert!(player.tick().is_none(), "不应重复推进队列");
        }
        let state = player.state();
        assert_eq!(state.position_ms, 0, "进度不得被旧曲尾部覆盖");
        assert_eq!(state.duration_ms, 0, "时长不得被旧曲尾部覆盖");
        assert_eq!(state.current_index, Some(1));
    }

    /// 新 URL 送达（play_url）后，恢复正常的进度同步。
    #[test]
    fn progress_sync_resumes_after_new_url() {
        let (mut player, engine) = setup(3);
        player.play_url("u0");
        engine.set(|i| {
            i.eof = true;
            i.pos_ms = 200_000;
            i.dur_ms = 200_000;
        });
        let pending = player.tick().expect("应返回待加载歌曲");
        assert_eq!(pending.id, 1);

        // 新文件加载：引擎开始播放，eof 变假、进度归零。
        player.play_url("u1");
        engine.set(|i| {
            i.eof = false;
            i.pos_ms = 1_000;
            i.dur_ms = 180_000;
        });
        assert!(player.tick().is_none());
        let state = player.state();
        assert_eq!(state.position_ms, 1_000, "加载后应恢复进度同步");
        assert_eq!(state.duration_ms, 180_000);
    }
}
