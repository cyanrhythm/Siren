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
    /// 等待加载窗口内收到的 seek：此时引擎仍指向**上一首**，直接 seek 会作用到旧文件
    /// 并让新曲从错误位置开始。记在这里，等 [`Player::play_url_from`] 加载完新文件后补发。
    pending_seek_ms: Option<i64>,
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
            pending_seek_ms: None,
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

    /// 当前曲目下标。
    pub fn current_index(&self) -> Option<usize> {
        self.queue.current_index()
    }

    /// 从队列移除指定下标（不重载引擎；调用方按需处理当前曲变化）。
    pub fn remove_at(&mut self, index: usize) {
        self.queue.remove(index);
        self.sync_queue();
        self.publish();
    }

    /// 队列内移动（拖动排序）。
    pub fn move_in_queue(&mut self, from: usize, to: usize) {
        self.queue.move_item(from, to);
        self.sync_queue();
        self.publish();
    }

    /// 清空队列，仅保留当前曲。
    pub fn clear_queue(&mut self) {
        self.queue.clear_keep_current();
        self.sync_queue();
        self.publish();
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
        // 新的一次恢复：清掉上一轮遗留的待补发 seek。
        self.pending_seek_ms = None;
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
        self.play_url_from(url, None);
    }

    /// 引擎加载并开始播放指定地址，可从 `start_ms` 起播（冷启动恢复上次进度）。
    pub fn play_url_from(&mut self, url: &str, start_ms: Option<i64>) {
        log::info!(
            "加载播放地址 song={:?} len={} start={:?}",
            self.state.song.as_ref().map(|s| s.id),
            url.len(),
            start_ms
        );
        let loaded = self.engine.load_from(url, start_ms);
        // 新文件已交给引擎（无论成败），之后读到的是新曲进度，可恢复位置/时长同步。
        // 新文件标志旧 EOF 已翻篇：解除手动切歌时的闩锁，允许本次播放自然结束再推进。
        self.awaiting_load = false;
        self.eof_latched = false;
        if loaded {
            self.engine.set_paused(false);
            log::debug!("引擎已加载新文件，恢复进度同步");
            self.state.playing = true;
            // 起播点：等待窗口内用户拖过的位置优先于调用方给的 `start_ms`
            //（前者更新，且代表用户最后一次意图）。
            let start = self.pending_seek_ms.or(start_ms);
            if let Some(ms) = start {
                let ms = ms.max(0);
                if self.pending_seek_ms.is_some() {
                    self.engine.seek_to_ms(ms);
                }
                self.state.position_ms = ms;
            }
            self.pending_seek_ms = None;
        } else {
            // 加载失败（含 mpv < 0.38 不识别 `-1` 参数）：不进入「播放中」的假死态。
            log::error!("引擎加载播放地址失败（检查 libmpv 版本，需 ≥ 0.38）: len={}", url.len());
            self.state.playing = false;
        }
        self.publish();
    }

    /// 取消在途的加载等待（URL 解析失败/放弃加载时调用）。
    ///
    /// 解除 [`awaiting_load`] 闩锁，恢复进度同步；否则状态机会一直停在「等待加载」，
    /// 表现为进度不动、EOF 也不再推进的卡死态。
    pub fn cancel_pending_load(&mut self) {
        if self.awaiting_load {
            log::debug!("取消在途加载等待，恢复进度同步");
            self.awaiting_load = false;
            self.publish();
        }
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
    ///
    /// 等待新文件加载期间（[`Player::restore`] 冷启动恢复或刚切歌后），引擎仍指向上一首，
    /// 此时**不能**把 seek 交给引擎——否则新曲会从错误位置开始。只更新界面位置并记为
    /// 待补发，等 [`Player::play_url_from`] 加载完成后作用于新文件。
    pub fn seek_ms(&mut self, position_ms: i64) {
        let ms = position_ms.max(0);
        if self.awaiting_load {
            log::debug!("等待新文件加载期间收到 seek={ms}ms，延后到加载完成后补发");
            self.pending_seek_ms = Some(ms);
            self.state.position_ms = ms;
            self.publish();
            return;
        }
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
        // 与进度同步同门控：等待新文件加载期间（awaiting_load）引擎仍报旧曲的 eof，
        // 若此时推进会造成旧文件播完后的连环自动切歌。
        if !self.awaiting_load && self.state.song.is_some() && eof && !self.eof_latched {
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
        // 切歌后上一首的待补发 seek 不再适用。
        self.pending_seek_ms = None;
        // 同步闩锁：若引擎此刻仍报旧曲的 eof（上一首刚播完），手动切歌后不应再被
        // 这个陈旧 eof 推进一步（否则会跳掉一首）；新文件播放（eof 变假）时自动解除。
        self.eof_latched = true;
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
        self.pending_seek_ms = None;
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
        fn load(&self, url: &str) -> bool {
            self.set(|i| {
                i.url = Some(url.to_string());
                i.eof = false;
            });
            true
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
    fn cancel_pending_load_resumes_progress_sync() {
        let (mut player, engine) = setup(3);
        // 手动切歌后进入等待加载窗口，期间不同步引擎进度。
        assert_eq!(player.next_track().unwrap().id, 1);
        engine.set(|i| i.pos_ms = 5_000);
        assert!(player.tick().is_none());
        assert_eq!(player.state().position_ms, 0, "等待加载期间不同步进度");
        // 解析失败：取消等待加载，恢复进度同步（不再卡死）。
        player.cancel_pending_load();
        assert!(player.tick().is_none());
        assert_eq!(player.state().position_ms, 5_000);
    }

    #[test]
    fn manual_selection_latches_stale_eof() {
        let (mut player, engine) = setup(3);
        // 引擎仍报告上一首已播完（尚未被 tick 处理）。
        engine.set(|i| i.eof = true);
        // 手动切到第 3 首：应同步闩锁 eof。
        assert_eq!(player.play_at(2).unwrap().id, 2);
        // 陈旧的 eof 不应再触发一次自动切歌。
        assert!(player.tick().is_none());
        assert_eq!(player.state().current_index, Some(2));
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
        // 先加载：`set_queue` 会进入「等待加载」窗口，此窗口内 seek 是延后补发的（见下个测试）。
        player.play_url("u");
        player.seek_ms(12_000);
        assert_eq!(player.state().position_ms, 12_000);
        assert_eq!(engine.0.lock().unwrap().pos_ms, 12_000);
    }

    #[test]
    fn seek_during_pending_load_defers_to_new_file() {
        let (mut player, engine) = setup(2);
        // 进入等待加载窗口（冷启动恢复语义），引擎此刻仍指向上一首。
        player.restore(songs(2), 1, PlayMode::ListLoop, 0, 0, None, None);
        engine.set(|i| i.pos_ms = 999);

        player.seek_ms(45_000);
        assert_eq!(
            engine.0.lock().unwrap().pos_ms,
            999,
            "等待加载期间不得把 seek 打到旧文件上"
        );
        assert_eq!(player.state().position_ms, 45_000, "界面位置应立即跟手");

        // 新文件送达后补发 seek。
        player.play_url("http://new");
        assert_eq!(
            engine.0.lock().unwrap().pos_ms,
            45_000,
            "加载完成后应把延后的 seek 作用于新文件"
        );
        assert_eq!(player.state().position_ms, 45_000);
    }

    #[test]
    fn deferred_seek_is_dropped_when_track_changes() {
        let (mut player, engine) = setup(3);
        player.restore(songs(3), 0, PlayMode::ListLoop, 0, 0, None, None);
        player.seek_ms(30_000);
        // 切到下一首：上一首的待补发 seek 不再适用。
        player.next_track();
        engine.set(|i| i.pos_ms = 0);
        player.play_url("http://next");
        assert_eq!(
            engine.0.lock().unwrap().pos_ms,
            0,
            "旧曲遗留的待补发 seek 不应作用到新曲"
        );
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
        // 手动选曲后需真正加载（URL 送达）再等自然结束。
        player.play_url("u1");
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
