//! 可复用组件：侧栏、播放条。
//!
//! 视图方法以 `impl Root` 分文件承载（同一 crate 内允许）。

mod collect;
mod comments;
mod like;
mod now_playing;
mod player_bar;
mod queue_layout;
mod queue_panel;
mod title_bar;

pub(crate) use collect::{overlay_close, overlay_focus, overlay_scrim, overlay_scrim_strong};
pub(crate) use like::like_button;
