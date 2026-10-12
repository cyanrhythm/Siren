//! 页面级组件（各自为独立 `Entity`）。

mod album;
mod home;
mod library;
mod playlist;
mod recent;
mod search;
mod settings;

pub(crate) use album::AlbumPage;
pub(crate) use home::HomePage;
pub(crate) use library::LibraryPage;
pub(crate) use playlist::PlaylistPage;
pub(crate) use recent::RecentPage;
pub(crate) use search::SearchPage;
