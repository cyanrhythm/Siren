//! 页面级组件（各自为独立 `Entity`）。

/// 跨页共享的列表尺寸（曲目行 / 序号列 / 时长列 / 封面等），避免魔法数字散落。
pub(crate) mod dim {
    /// 详情页曲目行高（歌单/专辑，含 40px 封面）。
    pub(crate) const ROW_H: f32 = 52.0;
    /// 紧凑曲目行高（歌手热门 / 搜索 / 最近播放）。
    pub(crate) const ROW_H_SM: f32 = 48.0;
    /// 歌单 / 专辑 / 艺人列表行高（48px 封面）。
    pub(crate) const ROW_H_LIST: f32 = 56.0;
    /// 序号列宽。
    pub(crate) const INDEX_W: f32 = 28.0;
    /// 时长列宽。
    pub(crate) const DURATION_W: f32 = 56.0;
    /// 歌曲行封面边长。
    pub(crate) const COVER_SONG: f32 = 40.0;
    /// 列表行封面边长（歌单/专辑/艺人）。
    pub(crate) const COVER_LIST: f32 = 48.0;
    /// 圆形小按钮（返回/收藏等）尺寸。
    pub(crate) const ROUND_BTN: f32 = 30.0;
}

mod account;
mod album;
mod artist;
mod home;
mod library;
mod playlist;
mod profile;
mod recent;
mod search;
mod settings;

pub(crate) use account::AccountPage;
pub(crate) use album::AlbumPage;
pub(crate) use artist::ArtistPage;
pub(crate) use home::HomePage;
pub(crate) use library::LibraryPage;
pub(crate) use playlist::PlaylistPage;
pub(crate) use profile::ProfilePage;
pub(crate) use recent::RecentPage;
pub(crate) use search::SearchPage;
