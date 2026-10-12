//! 网易云 API 端点与主机常量。

/// 根域名（WEAPI 与网页登录走此域）。
pub const WEB_HOST: &str = "music.163.com";
/// WEAPI 基址。
pub const WEB_BASE_URL: &str = "https://music.163.com";
/// EAPI 主机（绕开 PC 端风控）。
pub const EAPI_HOST: &str = "interface.music.163.com";
/// EAPI 基址。
pub const EAPI_BASE_URL: &str = "https://interface.music.163.com";
/// XEAPI 主机（写操作）。
pub const XEAPI_HOST: &str = "interface3.music.163.com";

/// API 路径常量。
pub mod path {
    /// 综合搜索（经典接口，可匿名）。
    ///
    /// 注意：`/weapi/cloudsearch/get/web` 匿名访问会被风控返回 `50000005`。
    pub const SEARCH: &str = "/weapi/search/get";
    /// 单曲播放地址（WEAPI）。
    pub const SONG_URL: &str = "/weapi/song/enhance/player/url/v1";
    /// 单曲播放地址（EAPI）。
    pub const SONG_URL_EAPI: &str = "/eapi/song/enhance/player/url/v1";
    /// 账号信息。
    pub const ACCOUNT: &str = "/weapi/w/nuser/account/get";
    /// 推荐歌单。
    pub const PERSONALIZED_PLAYLISTS: &str = "/weapi/personalized/playlist";
    /// 歌单详情。
    pub const PLAYLIST_DETAIL: &str = "/weapi/v6/playlist/detail";
    /// 用户歌单。
    pub const USER_PLAYLIST: &str = "/weapi/user/playlist";
    /// 二维码登录：申请 unikey。
    pub const QR_UNIKEY: &str = "/weapi/login/qrcode/unikey";
    /// 二维码登录：轮询扫码状态。
    pub const QR_CHECK: &str = "/weapi/login/qrcode/client/login";
    /// 喜欢 / 取消喜欢单曲（EAPI）。
    pub const SONG_LIKE: &str = "/eapi/song/like";
    /// 已红心歌曲 ID 列表（EAPI）。
    pub const SONG_LIKE_GET: &str = "/eapi/song/like/get";
}
