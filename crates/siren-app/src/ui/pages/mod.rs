//! 页面级组件：首页 / 音乐库 / 搜索 / 歌单详情。
//!
//! 视图方法以 `impl Root` 分文件承载（同一 crate 内允许），状态见 [`super::Root`]。

mod home;
mod library;
mod playlist;
mod search;
