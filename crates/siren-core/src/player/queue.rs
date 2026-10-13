//! 播放队列与播放模式（纯逻辑，可单测）。
//!
//! 队列只管理「顺序、当前索引、模式」；实际解码由音频引擎负责。

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::model::Song;

/// 播放模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlayMode {
    /// 列表循环。
    #[default]
    ListLoop,
    /// 单曲循环。
    SingleLoop,
    /// 随机播放。
    Shuffle,
}

/// 队列来源容器（供「正在播放」显示名称/跳转，并作为打卡上报的容器 id）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaySource {
    pub kind: PlaySourceKind,
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub cover_url: String,
}

/// 来源容器类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlaySourceKind {
    Playlist,
    Album,
}

/// 播放队列。
#[derive(Debug, Default)]
pub struct PlayerQueue {
    items: Vec<Song>,
    current: Option<usize>,
    mode: PlayMode,
    source: Option<PlaySource>,
    context: Option<String>,
    /// 进入特殊上下文（心动/漫游）前的队列备份。
    snapshot: Option<Snapshot>,
}

/// 入特殊上下文前的队列备份。
#[derive(Debug, Clone)]
struct Snapshot {
    items: Vec<Song>,
    index: usize,
    mode: PlayMode,
    source: Option<PlaySource>,
    context: Option<String>,
}

impl PlayerQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用新列表替换队列，并把当前项指向首曲。
    pub fn set_items(&mut self, items: Vec<Song>) {
        self.current = (!items.is_empty()).then_some(0);
        self.items = items;
    }

    pub fn items(&self) -> &[Song] {
        &self.items
    }

    /// 队列来源容器。
    pub fn source(&self) -> Option<&PlaySource> {
        self.source.as_ref()
    }

    pub fn set_source(&mut self, source: Option<PlaySource>) {
        self.source = source;
    }

    /// 播放上下文标签（如 `intelligence` / `similar_roaming`）。
    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    pub fn set_context(&mut self, context: Option<String>) {
        self.context = context;
    }

    /// 备份当前队列与上下文（进入心动/漫游前调用）。
    pub fn take_snapshot(&mut self) {
        self.snapshot = Some(Snapshot {
            items: self.items.clone(),
            index: self.current.unwrap_or(0),
            mode: self.mode,
            source: self.source.clone(),
            context: self.context.clone(),
        });
    }

    /// 是否存在可还原的备份。
    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    /// 丢弃备份（正常播放新队列时）。
    pub fn clear_snapshot(&mut self) {
        self.snapshot = None;
    }

    /// 退出特殊上下文：有备份则还原，否则仅清除上下文标签。
    pub fn exit_special_context(&mut self) {
        let Some(snapshot) = self.snapshot.take() else {
            self.context = None;
            return;
        };
        // 还原前记下正在播放的曲目，尽量把它定位到还原后的队列
        let playing_id = self.current.and_then(|i| self.items.get(i)).map(|s| s.id);
        self.items = snapshot.items;
        self.mode = snapshot.mode;
        self.source = snapshot.source;
        self.context = snapshot.context;
        self.current = playing_id
            .and_then(|id| self.items.iter().position(|s| s.id == id))
            .or_else(|| (!self.items.is_empty()).then_some(snapshot.index.min(self.items.len() - 1)));
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&Song> {
        self.current.and_then(|i| self.items.get(i))
    }

    pub fn mode(&self) -> PlayMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.mode = mode;
    }

    /// 选中指定下标；越界返回 `false`（队列不变）。
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.current = Some(index);
            true
        } else {
            false
        }
    }

    /// 手动「下一首」：始终顺序 +1 并环形回绕。
    pub fn next_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        Some(match self.current {
            Some(i) if i < last => i + 1,
            Some(_) => 0,
            None => 0,
        })
    }

    /// 手动「上一首」：顺序 -1 并环形回绕。
    pub fn previous_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        Some(match self.current {
            Some(0) | None => last,
            Some(i) => i - 1,
        })
    }

    /// 自然播完后应切换到的下标（遵循播放模式）。
    pub fn auto_next_index(&self) -> Option<usize> {
        match self.mode {
            PlayMode::SingleLoop => self.current,
            PlayMode::Shuffle => self.random_index(),
            PlayMode::ListLoop => self.next_index(),
        }
    }

    /// 用 `items` 替换 `index` 之后（含后续）的曲目（漫游续接：保留到当前，尾部续接相似歌曲）。
    pub fn append_after(&mut self, index: usize, items: Vec<Song>) {
        if self.items.is_empty() {
            return;
        }
        let index = index.min(self.items.len() - 1);
        self.items.truncate(index + 1);
        self.items.extend(items);
    }

    /// 按模式推进到下一首，返回新的当前曲。
    pub fn advance(&mut self) -> Option<&Song> {
        if let Some(i) = self.auto_next_index() {
            self.current = Some(i);
        }
        self.current()
    }

    fn random_index(&self) -> Option<usize> {
        match self.items.len() {
            0 => None,
            1 => Some(0),
            len => {
                let mut rng = rand::thread_rng();
                let mut idx = rng.gen_range(0..len);
                if Some(idx) == self.current {
                    idx = (idx + 1) % len;
                }
                Some(idx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: i64) -> Song {
        Song {
            id,
            name: format!("song{id}"),
            artists: String::new(),
            album: String::new(),
            cover_url: String::new(),
            duration_ms: 0,
            publish_time: 0,
            artist_ids: Vec::new(),
        }
    }

    fn queue_with(n: i64) -> PlayerQueue {
        let mut q = PlayerQueue::new();
        q.set_items((0..n).map(song).collect());
        q
    }

    #[test]
    fn empty_queue_has_no_current() {
        let q = PlayerQueue::new();
        assert_eq!(q.current_index(), None);
        assert!(q.next_index().is_none());
        assert!(q.previous_index().is_none());
    }

    #[test]
    fn set_items_selects_first() {
        let q = queue_with(3);
        assert_eq!(q.current_index(), Some(0));
        assert_eq!(q.current().unwrap().id, 0);
    }

    #[test]
    fn next_wraps_around() {
        let mut q = queue_with(3);
        assert_eq!(q.next_index(), Some(1));
        q.select(2);
        assert_eq!(q.next_index(), Some(0));
    }

    #[test]
    fn previous_wraps_around() {
        let mut q = queue_with(3);
        assert_eq!(q.previous_index(), Some(2));
        q.select(1);
        assert_eq!(q.previous_index(), Some(0));
    }

    #[test]
    fn select_out_of_range_fails() {
        let mut q = queue_with(2);
        assert!(!q.select(5));
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn single_loop_auto_stays() {
        let mut q = queue_with(3);
        q.select(1);
        q.set_mode(PlayMode::SingleLoop);
        assert_eq!(q.auto_next_index(), Some(1));
    }

    #[test]
    fn shuffle_advances_within_range_and_changes() {
        let mut q = queue_with(3);
        q.select(1);
        q.set_mode(PlayMode::Shuffle);
        for _ in 0..20 {
            let idx = q.auto_next_index().unwrap();
            assert!(idx < 3);
            assert_ne!(idx, 1, "随机播放应换一首");
        }
    }

    #[test]
    fn list_loop_advance_moves_forward() {
        let mut q = queue_with(3);
        q.set_mode(PlayMode::ListLoop);
        assert_eq!(q.advance().unwrap().id, 1);
        assert_eq!(q.advance().unwrap().id, 2);
        assert_eq!(q.advance().unwrap().id, 0);
    }
}
