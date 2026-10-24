//! 可复用组件：侧栏、播放条。
//!
//! 视图方法以 `impl Root` 分文件承载（同一 crate 内允许）。

mod buttons;
mod collect;
mod comments;
mod detail_shell;
mod like;
mod now_playing;
mod offline_hint;
mod player_bar;
mod queue_layout;
mod queue_panel;
mod song_row;
mod title_bar;

pub(crate) use buttons::{icon_button, play_circle};
pub(crate) use collect::{overlay_close, overlay_focus, overlay_scrim, overlay_scrim_strong};
pub(crate) use detail_shell::{detail_shell, sticky_bar, StickyBar};
pub(crate) use like::like_button;
pub(crate) use offline_hint::offline_hint;
pub(crate) use song_row::{song_row, IndexCell, SongRow};
