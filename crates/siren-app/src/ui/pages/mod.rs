//! 页面级组件（各自为独立 `Entity`）。

mod home;
mod library;
mod playlist;
mod search;

pub(crate) use home::HomePage;
pub(crate) use library::LibraryPage;
pub(crate) use playlist::PlaylistPage;
pub(crate) use search::SearchPage;
