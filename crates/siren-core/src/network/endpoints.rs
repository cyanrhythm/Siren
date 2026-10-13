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
    /// 云搜索（EAPI，按 type 区分单曲/专辑/歌手/歌单/电台）。
    pub const CLOUD_SEARCH: &str = "/eapi/cloudsearch/pc";
    /// 最近播放歌曲（WEAPI，需登录）。
    pub const RECENT_SONGS: &str = "/weapi/play-record/song/list";
    /// 最近播放歌单（EAPI，需登录）。
    pub const RECENT_PLAYLISTS: &str = "/eapi/play-record/playlist/list";
    /// 最近播放专辑（WEAPI，需登录）。
    pub const RECENT_ALBUMS: &str = "/weapi/play-record/album/list";
    /// 打卡上报（EAPI）。
    pub const WEBLOG: &str = "/eapi/feedback/weblog";
    /// 歌曲详情（EAPI，`c=[{"id":..}]`）。
    pub const SONG_DETAIL: &str = "/eapi/v3/song/detail";
    /// 歌词（EAPI，含 lrc/tlyric/yrc）。
    pub const LYRIC: &str = "/eapi/song/lyric/v1";
    /// 刷新登录态（WEAPI，新 Cookie 在 Set-Cookie）。
    pub const LOGIN_REFRESH: &str = "/weapi/login/token/refresh";
    /// 歌曲百科摘要（WEAPI）。
    pub const SONG_WIKI: &str = "/weapi/song/play/about/block/page";
    /// 歌曲制作人员（WEAPI）。
    pub const SONG_CREATORS: &str = "/weapi/song/creators";
    /// 评论列表（WEAPI v2）。
    pub const COMMENTS_V2: &str = "/weapi/v2/resource/comments";
    /// 评论点赞/取消（WEAPI，`{op}` = like / unlike）。
    pub const COMMENT_LIKE: &str = "/weapi/v1/comment/";
    /// 评论楼层（WEAPI）。
    pub const COMMENT_FLOOR: &str = "/weapi/resource/comment/floor/get";
    /// 歌手详情（EAPI）。
    pub const ARTIST_DETAIL: &str = "/eapi/artist/head/info/get";
    /// 歌手专辑（WEAPI，后接 `{id}`）。
    pub const ARTIST_ALBUMS: &str = "/weapi/artist/albums/";
    /// 相似歌手（EAPI）。
    pub const ARTIST_SIMI: &str = "/eapi/discovery/simiArtist";
    /// 歌手关注/粉丝数（WEAPI）。
    pub const ARTIST_FOLLOW_COUNT: &str = "/weapi/artist/follow/count/get";
    /// 收藏/取消收藏歌手（WEAPI，后接 `sub` / `unsub`）。
    pub const ARTIST_SUB: &str = "/weapi/artist/";
    /// 相似歌曲（EAPI）。
    pub const SIMI_SONG: &str = "/eapi/v1/discovery/simiSong";
    /// 心动模式推荐（EAPI）。
    pub const INTELLIGENCE: &str = "/eapi/playmode/intelligence/list";
    /// 专辑详情（WEAPI，后接 `{id}`）。
    pub const ALBUM_DETAIL: &str = "/weapi/v1/album/";
    /// 用户详情（WEAPI，后接 `{uid}`）。
    pub const USER_DETAIL: &str = "/weapi/v1/user/detail/";
    /// 关注用户（WEAPI，后接 `{id}`）。
    pub const USER_FOLLOW: &str = "/weapi/user/follow/";
    /// 取消关注用户（WEAPI，后接 `{id}`）。
    pub const USER_DELFOLLOW: &str = "/weapi/user/delfollow/";
    /// 用户关注列表（WEAPI，后接 `{uid}`）。
    pub const USER_FOLLOWS: &str = "/weapi/user/getfollows/";
    /// 用户粉丝列表（WEAPI，后接 `{uid}`）。
    pub const USER_FOLLOWEDS: &str = "/weapi/user/getfolloweds/";
    /// 收藏专辑列表（EAPI）。
    pub const ALBUM_SUBLIST: &str = "/eapi/album/sublist";
    /// 更新用户歌单排序（WEAPI）。
    pub const PLAYLIST_ORDER: &str = "/weapi/playlist/order/update";
    /// 置顶/取消置顶自建歌单（WEAPI，后接 `top` / `top/cancel`）。
    pub const PLAYLIST_TOP: &str = "/weapi/user/playlist/top";
    /// 收藏歌手列表（EAPI）。
    pub const ARTIST_SUBLIST: &str = "/eapi/artist/sublist";
    /// 歌手热门歌曲（WEAPI）。
    pub const ARTIST_TOP_SONGS: &str = "/weapi/artist/top/song";
    /// 歌单增删歌曲（EAPI）。
    pub const PLAYLIST_MANIPULATE: &str = "/eapi/playlist/manipulate/tracks";
    /// 歌单收藏/取消（EAPI，后接 `subscribe` / `unsubscribe`）。
    pub const PLAYLIST_SUB: &str = "/eapi/playlist/";
    /// 专辑收藏/取消（WEAPI，后接 `sub` / `unsub`）。
    pub const ALBUM_SUB: &str = "/weapi/album/";
    /// 歌单改名（EAPI）。
    pub const PLAYLIST_UPDATE_NAME: &str = "/eapi/playlist/update/name";
    /// 歌单删除（WEAPI）。
    pub const PLAYLIST_REMOVE: &str = "/weapi/playlist/remove";
    /// 新建歌单（EAPI）。
    pub const PLAYLIST_CREATE: &str = "/eapi/playlist/create";
    /// 更新歌单简介（EAPI）。
    pub const PLAYLIST_UPDATE_DESC: &str = "/eapi/playlist/desc/update";
    /// 歌曲详情（EAPI 批量，`c=[{"id":..}]`）。
    pub const SONG_DETAIL_V3: &str = "/eapi/v3/song/detail";
    /// 申请 NOS 上传凭证（WEAPI）。
    pub const NOS_TOKEN_ALLOC: &str = "/weapi/nos/token/alloc";
    /// 登记歌单封面（WEAPI）。
    pub const PLAYLIST_COVER_UPDATE: &str = "/weapi/playlist/cover/update";
}
