//! 可复用组件：侧栏、播放条。
//!
//! 视图方法以 `impl Root` 分文件承载（同一 crate 内允许）。

mod collect;
mod comments;
mod now_playing;
mod player_bar;
mod title_bar;
