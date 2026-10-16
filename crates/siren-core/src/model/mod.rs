//! 数据模型：API 返回结构与领域实体。
//!
//! 对应 Melodia 的 `core/model/`。当前落地播放/搜索所需的子集，
//! 后续按业务域（歌单、专辑、歌手、播客等）扩展。

pub mod quality;
pub mod wiki;

pub use quality::AudioQuality;
pub use wiki::{parse_song_wiki, CreatorRole, MusicMemory, SongWiki};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 歌曲。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Song {
    pub id: i64,
    pub name: String,
    /// 以 `/` 拼接的歌手名。
    pub artists: String,
    pub album: String,
    /// 专辑封面地址（可能为空）。
    pub cover_url: String,
    /// 时长（毫秒），源头可能缺失。
    pub duration_ms: i64,
    /// 发行时间戳（毫秒），可能为 0。
    pub publish_time: i64,
    /// 歌手 ID 列表（用于歌手详情跳转）。
    pub artist_ids: Vec<i64>,
}

/// 歌单（列表项）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub track_count: i64,
    pub play_count: i64,
}

/// 专辑（搜索结果项）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    /// 以 `/` 拼接的歌手名。
    pub artists: String,
}

/// 歌手（搜索结果项）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
}

/// 歌手详情。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtistDetail {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub brief_desc: String,
    pub album_size: i64,
    pub music_size: i64,
}

/// 歌手专辑。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ArtistAlbum {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub publish_time: i64,
    pub size: i64,
}

/// 评论用户。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommentUser {
    pub user_id: i64,
    pub nickname: String,
    pub avatar_url: String,
}

/// 单条评论。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommentItem {
    pub comment_id: i64,
    pub user: CommentUser,
    pub content: String,
    pub time: i64,
    pub liked_count: i64,
    pub liked: bool,
    pub reply_count: i64,
}

/// 评论分页。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentPage {
    pub comments: Vec<CommentItem>,
    pub total: i64,
    pub has_more: bool,
    pub cursor: String,
}

/// 歌单详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub tracks: Vec<Song>,
    /// 完整曲目 ID 顺序（详情 `tracks` 可能被截断，超出部分需据此补全）。
    pub track_ids: Vec<i64>,
    /// 当前用户是否已收藏。
    pub subscribed: bool,
    /// 创建者 uid（用于判断是否本人创建）。
    pub creator_id: i64,
    /// 创建者昵称。
    pub creator_nickname: String,
    /// 创建者头像。
    pub creator_avatar: String,
    /// 是否隐私歌单（10 为隐私）。
    pub privacy: i64,
    /// 曲目总数（详情可能截断，供展示）。
    pub track_count: i64,
}

/// 专辑详情。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AlbumDetail {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub description: String,
    /// 以 `/` 拼接的歌手名。
    pub artists: String,
    pub songs: Vec<Song>,
    /// 当前用户是否已收藏该专辑。
    pub subscribed: bool,
}

/// 解析推荐歌单（`/weapi/personalized/playlist`，`result` 数组）。
pub fn parse_personalized_playlists(value: &Value) -> Vec<Playlist> {
    value
        .get("result")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_playlist).collect())
        .unwrap_or_default()
}

fn parse_playlist(value: &Value) -> Playlist {
    Playlist {
        id: value.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(value, "name"),
        cover_url: value
            .get("picUrl")
            .or_else(|| value.get("coverImgUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        track_count: value.get("trackCount").and_then(Value::as_i64).unwrap_or_default(),
        play_count: value.get("playCount").and_then(Value::as_i64).unwrap_or_default(),
    }
}

/// 解析用户歌单（`/weapi/user/playlist`，`playlist` 数组）。
pub fn parse_user_playlists(value: &Value) -> Vec<Playlist> {
    value
        .get("playlist")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_playlist).collect())
        .unwrap_or_default()
}

/// 解析云搜索专辑结果（`result.albums`）。
pub fn parse_search_albums(value: &Value) -> Vec<Album> {
    value
        .get("result")
        .and_then(|r| r.get("albums"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_album).collect())
        .unwrap_or_default()
}

/// 解析云搜索歌手结果（`result.artists`）。
pub fn parse_search_artists(value: &Value) -> Vec<Artist> {
    value
        .get("result")
        .and_then(|r| r.get("artists"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_artist).collect())
        .unwrap_or_default()
}

/// 首页货架（服务端下发的一组同类卡片）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeShelf {
    pub block_code: String,
    pub title: String,
    /// 榜单类货架：卡片角标显示名次。
    pub show_rank: bool,
    pub cards: Vec<HomeCard>,
}

/// 货架卡片（只保留有落地页/可播的类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeCard {
    Playlist {
        id: i64,
        title: String,
        cover_url: String,
        caption: String,
    },
    Album {
        id: i64,
        title: String,
        cover_url: String,
        caption: String,
    },
    Song {
        id: i64,
        title: String,
        cover_url: String,
        caption: String,
    },
    /// 播客单集：`program_id` 仅作 key，播放走 `song_id`。
    Voice {
        program_id: i64,
        song_id: i64,
        title: String,
        cover_url: String,
        caption: String,
    },
}

impl HomeCard {
    pub fn title(&self) -> &str {
        match self {
            HomeCard::Playlist { title, .. }
            | HomeCard::Album { title, .. }
            | HomeCard::Song { title, .. }
            | HomeCard::Voice { title, .. } => title,
        }
    }
    pub fn cover_url(&self) -> &str {
        match self {
            HomeCard::Playlist { cover_url, .. }
            | HomeCard::Album { cover_url, .. }
            | HomeCard::Song { cover_url, .. }
            | HomeCard::Voice { cover_url, .. } => cover_url,
        }
    }
    pub fn caption(&self) -> &str {
        match self {
            HomeCard::Playlist { caption, .. }
            | HomeCard::Album { caption, .. }
            | HomeCard::Song { caption, .. }
            | HomeCard::Voice { caption, .. } => caption,
        }
    }
    /// 类型标签（角标文案）。
    pub fn type_label(&self) -> &'static str {
        match self {
            HomeCard::Playlist { .. } => "歌单",
            HomeCard::Album { .. } => "专辑",
            HomeCard::Song { .. } => "单曲",
            HomeCard::Voice { .. } => "播客",
        }
    }
}

fn extract_id(action: Option<&str>, prefix: &str) -> Option<i64> {
    let action = action?;
    let idx = action.find(prefix)?;
    let rest = &action[idx + prefix.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn home_caption(ui: &Value) -> String {
    if let Some(labels) = ui.get("labelTexts").and_then(Value::as_array) {
        let joined: Vec<&str> = labels
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .collect();
        if !joined.is_empty() {
            return joined.join(" · ");
        }
    }
    if let Some(text) = ui.get("labelText").and_then(|l| l.get("text")).and_then(Value::as_str) {
        if !text.is_empty() {
            return text.to_string();
        }
    }
    if let Some(text) = ui.get("subTitle").and_then(|t| t.get("title")).and_then(Value::as_str) {
        if !text.is_empty() {
            return text.to_string();
        }
    }
    ui.get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// 解析首页货架（`/eapi/homepage/block/page`，`data.blocks`）。
pub fn parse_home_shelves(value: &Value) -> Vec<HomeShelf> {
    const SKIP: [&str; 2] = ["HOMEPAGE_BANNER", "HOMEPAGE_BLOCK_OLD_DRAGON_BALL"];
    let Some(blocks) = value
        .get("data")
        .and_then(|d| d.get("blocks"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut shelves = Vec::new();
    for block in blocks {
        let block_code = block.get("blockCode").and_then(Value::as_str).unwrap_or_default();
        if SKIP.contains(&block_code) {
            continue;
        }
        let ui = block.get("uiElement");
        let title = ui
            .and_then(|u| u.get("mainTitle").and_then(|t| t.get("title")))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                ui.and_then(|u| u.get("subTitle").and_then(|t| t.get("title")))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_default()
            .to_string();
        if title.is_empty() {
            continue;
        }
        let mut cards = Vec::new();
        if let Some(creatives) = block.get("creatives").and_then(Value::as_array) {
            for creative in creatives {
                let Some(resources) = creative.get("resources").and_then(Value::as_array) else {
                    continue;
                };
                for res in resources {
                    let Some(rui) = res.get("uiElement") else { continue };
                    let name = rui
                        .get("mainTitle")
                        .and_then(|t| t.get("title"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let cover = rui
                        .get("image")
                        .and_then(|i| i.get("imageUrl"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if name.is_empty() || cover.is_empty() {
                        continue;
                    }
                    let caption = home_caption(rui);
                    let action = res.get("action").and_then(Value::as_str);
                    let resource_id = res
                        .get("resourceId")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<i64>().ok());
                    let kind = res
                        .get("resourceType")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_lowercase();
                    let card = match kind.as_str() {
                        "list" => {
                            extract_id(action, "orpheus://playlist/")
                                .or(resource_id)
                                .map(|id| HomeCard::Playlist {
                                    id,
                                    title: name.to_string(),
                                    cover_url: cover.to_string(),
                                    caption: caption.clone(),
                                })
                        }
                        "album" | "digitalalbum" => extract_id(action, "orpheus://album/").map(|id| HomeCard::Album {
                            id,
                            title: name.to_string(),
                            cover_url: cover.to_string(),
                            caption: caption.clone(),
                        }),
                        "song" => resource_id.map(|id| HomeCard::Song {
                            id,
                            title: name.to_string(),
                            cover_url: cover.to_string(),
                            caption: caption.clone(),
                        }),
                        "voice" => res
                            .get("resourceExtInfo")
                            .and_then(|e| e.get("djProgram"))
                            .and_then(|p| {
                                let song_id = p.get("mainSong").and_then(|s| s.get("id")).and_then(Value::as_i64)?;
                                let program_id = p.get("id").and_then(Value::as_i64).or(resource_id)?;
                                (song_id > 0).then_some((program_id, song_id))
                            })
                            .map(|(program_id, song_id)| HomeCard::Voice {
                                program_id,
                                song_id,
                                title: name.to_string(),
                                cover_url: cover.to_string(),
                                caption: caption.clone(),
                            }),
                        _ => None,
                    };
                    if let Some(card) = card {
                        cards.push(card);
                    }
                }
            }
        }
        if cards.is_empty() {
            continue;
        }
        shelves.push(HomeShelf {
            block_code: block_code.to_string(),
            title,
            show_rank: block_code == "HOMEPAGE_BLOCK_TOPLIST",
            cards,
        });
    }
    shelves
}

/// 用户详情。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserProfile {
    pub user_id: i64,
    pub nickname: String,
    pub avatar_url: String,
    pub signature: String,
    pub follows: i64,
    pub followeds: i64,
    pub playlist_count: i64,
    pub followed: bool,
}

/// 用户简要（关注/粉丝列表项）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserBrief {
    pub user_id: i64,
    pub nickname: String,
    pub avatar_url: String,
    pub signature: String,
    pub followed: bool,
}

fn parse_user_brief(v: &Value) -> UserBrief {
    UserBrief {
        user_id: v.get("userId").and_then(Value::as_i64).unwrap_or_default(),
        nickname: str_field(v, "nickname"),
        avatar_url: v
            .get("avatarUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        signature: str_field(v, "signature"),
        followed: v.get("followed").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// 解析用户详情（`profile`）。
pub fn parse_user_profile(value: &Value) -> Option<UserProfile> {
    let p = value.get("profile")?;
    let user_id = p.get("userId").and_then(Value::as_i64).unwrap_or_default();
    if user_id == 0 {
        return None;
    }
    Some(UserProfile {
        user_id,
        nickname: str_field(p, "nickname"),
        avatar_url: p
            .get("avatarUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        signature: str_field(p, "signature"),
        follows: p.get("follows").and_then(Value::as_i64).unwrap_or_default(),
        followeds: p.get("followeds").and_then(Value::as_i64).unwrap_or_default(),
        playlist_count: p.get("playlistCount").and_then(Value::as_i64).unwrap_or_default(),
        followed: p.get("followed").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// 解析关注/粉丝列表（取 `follow` 或 `followeds` 数组）。
pub fn parse_user_follow_list(value: &Value) -> Vec<UserBrief> {
    let arr = value
        .get("follow")
        .and_then(Value::as_array)
        .or_else(|| value.get("followeds").and_then(Value::as_array));
    arr.map(|items| items.iter().map(parse_user_brief).collect())
        .unwrap_or_default()
}

/// 用户等级信息（`/eapi/user/level`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserLevel {
    pub level: i64,
    pub now_play_count: i64,
    pub now_login_count: i64,
    pub next_play_count: i64,
    pub next_login_count: i64,
    pub progress: f64,
}

/// 解析用户等级（`data`）。
pub fn parse_user_level(value: &Value) -> Option<UserLevel> {
    let d = value.get("data")?;
    Some(UserLevel {
        level: d.get("level").and_then(Value::as_i64).unwrap_or_default(),
        now_play_count: d.get("nowPlayCount").and_then(Value::as_i64).unwrap_or_default(),
        now_login_count: d.get("nowLoginCount").and_then(Value::as_i64).unwrap_or_default(),
        next_play_count: d.get("nextPlayCount").and_then(Value::as_i64).unwrap_or_default(),
        next_login_count: d.get("nextLoginCount").and_then(Value::as_i64).unwrap_or_default(),
        progress: d.get("progress").and_then(Value::as_f64).unwrap_or_default(),
    })
}

/// VIP 状态信息（`/eapi/vip/info`）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VipInfo {
    /// 0-非 VIP，10-音乐包，11-黑胶 VIP。
    pub vip_type: i64,
    pub red_vip_level: i64,
    pub expire_time: i64,
    pub is_vip: bool,
}

/// 解析 VIP 信息（`data`）。
///
/// 真实结构以 `data.associator`（黑胶 VIP）/ `data.musicPackage`（音乐包）的 `expireTime` 为准，
/// `data.redVipLevel` 为黑胶等级；兼容旧的 `vipType` / `isVip` 字段。
pub fn parse_vip_info(value: &Value) -> Option<VipInfo> {
    let d = value.get("data")?;
    let now = d.get("now").and_then(Value::as_i64).unwrap_or(0);
    let alive = |exp: i64| exp > 0 && (now == 0 || exp > now);
    let assoc_exp = d
        .get("associator")
        .and_then(|a| a.get("expireTime"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let pkg_exp = d
        .get("musicPackage")
        .and_then(|a| a.get("expireTime"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let (vip_type, expire_time) = if alive(assoc_exp) {
        (11, assoc_exp)
    } else if alive(pkg_exp) {
        (10, pkg_exp)
    } else {
        (
            d.get("vipType").and_then(Value::as_i64).unwrap_or(0),
            d.get("expireTime").and_then(Value::as_i64).unwrap_or(0),
        )
    };
    Some(VipInfo {
        vip_type,
        red_vip_level: d.get("redVipLevel").and_then(Value::as_i64).unwrap_or(0),
        expire_time,
        is_vip: vip_type != 0 || d.get("isVip").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// 账号第三方绑定项（`/eapi/user/binding`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBinding {
    /// 1-手机，2-新浪微博，5-腾讯微博，10-微信，20-QQ，1000-网易邮箱。
    pub kind: i64,
    pub type_name: String,
    pub expired: bool,
}

/// 解析账号绑定列表（顶层 `bindings`）。
pub fn parse_bindings(value: &Value) -> Vec<AccountBinding> {
    value
        .get("bindings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|v| AccountBinding {
                    kind: v.get("type").and_then(Value::as_i64).unwrap_or_default(),
                    type_name: str_field(v, "typeName"),
                    expired: v.get("expired").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 最近播放歌曲（含播放时间）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentSong {
    pub song: Song,
    /// 播放时间（Unix 毫秒）。
    pub play_time: i64,
}

/// 最近播放歌单（含播放时间）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecentPlaylist {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub creator_name: String,
    pub play_time: i64,
}

/// 最近播放专辑（含播放时间）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecentAlbum {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub artist: String,
    pub play_time: i64,
}

/// 解析最近播放歌曲（`data.list[].{playTime,data}`）。
pub fn parse_recent_songs_with_time(value: &Value) -> Vec<RecentSong> {
    value
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data")?;
                    data.get("id")?;
                    Some(RecentSong {
                        song: parse_song(data),
                        play_time: item.get("playTime").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析最近播放歌单（`data.list[].{playTime,data}`）。
pub fn parse_recent_playlists(value: &Value) -> Vec<RecentPlaylist> {
    value
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data")?;
                    Some(RecentPlaylist {
                        id: data.get("id").and_then(Value::as_i64).unwrap_or_default(),
                        name: str_field(data, "name"),
                        cover_url: data
                            .get("coverImgUrl")
                            .or_else(|| data.get("picUrl"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        creator_name: data
                            .get("creator")
                            .and_then(|c| c.get("nickname"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        play_time: item.get("playTime").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析最近播放专辑（`data.list[].{playTime,data}`）。
pub fn parse_recent_albums(value: &Value) -> Vec<RecentAlbum> {
    value
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let data = item.get("data")?;
                    Some(RecentAlbum {
                        id: data.get("id").and_then(Value::as_i64).unwrap_or_default(),
                        name: str_field(data, "name"),
                        cover_url: data
                            .get("picUrl")
                            .or_else(|| data.get("coverImgUrl"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        artist: data
                            .get("artist")
                            .and_then(|a| a.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        play_time: item.get("playTime").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析收藏专辑（`/eapi/album/sublist`，`data` 数组）。
pub fn parse_album_sublist(value: &Value) -> Vec<Album> {
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_album).collect())
        .unwrap_or_default()
}

/// 解析收藏歌手（`/eapi/artist/sublist`，`data` 数组）。
pub fn parse_artist_sublist(value: &Value) -> Vec<Artist> {
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_artist).collect())
        .unwrap_or_default()
}

/// 解析歌手热门歌曲（`/weapi/artist/top/song`，`songs` 数组）。
pub fn parse_artist_top_songs(value: &Value) -> Vec<Song> {
    value
        .get("songs")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_song).collect())
        .unwrap_or_default()
}

/// 解析云搜索歌单结果（`result.playlists`）。
pub fn parse_search_playlists(value: &Value) -> Vec<Playlist> {
    value
        .get("result")
        .and_then(|r| r.get("playlists"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_playlist).collect())
        .unwrap_or_default()
}

fn parse_album(value: &Value) -> Album {
    let artists = value
        .get("artists")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    Album {
        id: value.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(value, "name"),
        cover_url: value
            .get("picUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        artists,
    }
}

fn parse_artist(value: &Value) -> Artist {
    Artist {
        id: value.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(value, "name"),
        cover_url: value
            .get("picUrl")
            .or_else(|| value.get("img1v1Url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

/// 解析歌手详情（`data.artist`）。
pub fn parse_artist_detail(value: &Value) -> Option<ArtistDetail> {
    let a = value.get("data").and_then(|d| d.get("artist"))?;
    Some(ArtistDetail {
        id: a.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(a, "name"),
        cover_url: ["cover", "avatar", "picUrl", "img1v1Url"]
            .iter()
            .find_map(|k| a.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
            .unwrap_or_default()
            .to_string(),
        brief_desc: str_field(a, "briefDesc"),
        album_size: a.get("albumSize").and_then(Value::as_i64).unwrap_or_default(),
        music_size: a.get("musicSize").and_then(Value::as_i64).unwrap_or_default(),
    })
}

/// 解析歌手专辑（`hotAlbums`）。
pub fn parse_artist_albums(value: &Value) -> Vec<ArtistAlbum> {
    value
        .get("hotAlbums")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|a| ArtistAlbum {
                    id: a.get("id").and_then(Value::as_i64).unwrap_or_default(),
                    name: str_field(a, "name"),
                    cover_url: a.get("picUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
                    publish_time: a.get("publishTime").and_then(Value::as_i64).unwrap_or_default(),
                    size: a.get("size").and_then(Value::as_i64).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析相似歌手（`artists`）。
pub fn parse_simi_artists(value: &Value) -> Vec<Artist> {
    value
        .get("artists")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_artist).collect())
        .unwrap_or_default()
}

/// 解析单条评论。
pub fn parse_comment_item(value: &Value) -> CommentItem {
    let user = value.get("user").unwrap_or(&Value::Null);
    CommentItem {
        comment_id: value.get("commentId").and_then(Value::as_i64).unwrap_or_default(),
        user: CommentUser {
            user_id: user.get("userId").and_then(Value::as_i64).unwrap_or_default(),
            nickname: str_field(user, "nickname"),
            avatar_url: str_field(user, "avatarUrl"),
        },
        content: str_field(value, "content"),
        time: value.get("time").and_then(Value::as_i64).unwrap_or_default(),
        liked_count: value.get("likedCount").and_then(Value::as_i64).unwrap_or_default(),
        liked: value.get("liked").and_then(Value::as_bool).unwrap_or(false),
        reply_count: value.get("replyCount").and_then(Value::as_i64).unwrap_or_default(),
    }
}

/// 解析评论分页（`/weapi/v2/resource/comments` 的 `data`）。
pub fn parse_comments_v2(value: &Value) -> CommentPage {
    let data = value.get("data").unwrap_or(&Value::Null);
    let comments = data
        .get("comments")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_comment_item).collect())
        .unwrap_or_default();
    CommentPage {
        comments,
        total: data.get("totalCount").and_then(Value::as_i64).unwrap_or_default(),
        has_more: data.get("hasMore").and_then(Value::as_bool).unwrap_or(false),
        cursor: str_field(data, "cursor"),
    }
}

/// 解析歌单详情（`/weapi/v6/playlist/detail`，`playlist` 对象）。
pub fn parse_playlist_detail(value: &Value) -> Option<PlaylistDetail> {
    let playlist = value.get("playlist")?;
    let tracks = playlist
        .get("tracks")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_song).collect())
        .unwrap_or_default();
    let track_ids = playlist
        .get("trackIds")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|t| t.get("id").and_then(Value::as_i64)).collect())
        .unwrap_or_default();
    Some(PlaylistDetail {
        id: playlist.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(playlist, "name"),
        cover_url: playlist
            .get("coverImgUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tracks,
        track_ids,
        subscribed: playlist.get("subscribed").and_then(Value::as_bool).unwrap_or(false),
        track_count: playlist.get("trackCount").and_then(Value::as_i64).unwrap_or_default(),
        creator_id: playlist
            .get("creator")
            .and_then(|c| c.get("userId"))
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        creator_nickname: playlist
            .get("creator")
            .and_then(|c| c.get("nickname"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        creator_avatar: playlist
            .get("creator")
            .and_then(|c| c.get("avatarUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        privacy: playlist.get("privacy").and_then(Value::as_i64).unwrap_or_default(),
    })
}

/// 解析专辑详情（`/weapi/v1/album/{id}`）。
///
/// 收藏态取自**顶层 `resourceState`**——实测该接口既不返回顶层 `subscribed`，
/// 也不返回 `album.subscribed`（已收藏专辑 5/5 为 `resourceState: true`，
/// 未收藏专辑该字段整体缺失）。`album.subscribed` 仅作兼容回退。
pub fn parse_album_detail(value: &Value) -> Option<AlbumDetail> {
    let album = value.get("album")?;
    let artists = album
        .get("artists")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let songs = value
        .get("songs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_song).collect())
        .unwrap_or_default();
    Some(AlbumDetail {
        id: album.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(album, "name"),
        cover_url: album
            .get("picUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        description: album
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        artists,
        songs,
        subscribed: value
            .get("resourceState")
            .and_then(Value::as_bool)
            .or_else(|| album.get("subscribed").and_then(Value::as_bool))
            .unwrap_or(false),
    })
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// 解析 `search/get` 与 `cloudsearch/get/web` 两种 schema 的单曲列表。
pub fn parse_search_songs(value: &Value) -> Vec<Song> {
    value
        .get("result")
        .and_then(|r| r.get("songs"))
        .and_then(Value::as_array)
        .map(|songs| songs.iter().map(parse_song).collect())
        .unwrap_or_default()
}

/// 解析单曲（兼容经典 `artists/album/duration` 与 `ar/al/dt`）。
pub fn parse_song(value: &Value) -> Song {
    let id = value.get("id").and_then(Value::as_i64).unwrap_or_default();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // 兼容经典 search/get（artists/album/duration）与 cloudsearch（ar/al/dt）
    let artist_list = value
        .get("artists")
        .or_else(|| value.get("ar"))
        .and_then(Value::as_array);
    let artists = artist_list
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let artist_ids = artist_list
        .map(|arr| arr.iter().filter_map(|a| a.get("id").and_then(Value::as_i64)).collect())
        .unwrap_or_default();

    let album = value
        .get("album")
        .or_else(|| value.get("al"))
        .and_then(|al| al.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let cover_url = value
        .get("al")
        .or_else(|| value.get("album"))
        .and_then(|al| al.get("picUrl"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let duration_ms = value
        .get("duration")
        .or_else(|| value.get("dt"))
        .and_then(Value::as_i64)
        .unwrap_or_default();

    let publish_time = value.get("publishTime").and_then(Value::as_i64).unwrap_or_default();

    Song {
        id,
        name,
        artists,
        album,
        cover_url,
        duration_ms,
        publish_time,
        artist_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_classic_schema() {
        let value = json!({
            "result": { "songs": [
                { "id": 1, "name": "歌", "artists": [{"name": "甲"}, {"name": "乙"}],
                  "album": {"name": "专", "picUrl": "http://c/1.jpg"}, "duration": 123000 }
            ]}
        });
        let songs = parse_search_songs(&value);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].artists, "甲/乙");
        assert_eq!(songs[0].album, "专");
        assert_eq!(songs[0].cover_url, "http://c/1.jpg");
        assert_eq!(songs[0].duration_ms, 123000);
    }

    #[test]
    fn parses_cloudsearch_schema() {
        let value = json!({
            "result": { "songs": [
                { "id": 2, "name": "x", "ar": [{"name": "a"}], "al": {"name": "b"}, "dt": 5000 }
            ]}
        });
        let songs = parse_search_songs(&value);
        assert_eq!(songs[0].artists, "a");
        assert_eq!(songs[0].duration_ms, 5000);
    }

    #[test]
    fn missing_fields_are_default() {
        let songs = parse_search_songs(&json!({}));
        assert!(songs.is_empty());
    }

    #[test]
    fn album_subscribed_comes_from_top_level_resource_state() {
        // 已收藏：接口只在顶层给 resourceState，`album` 内没有 subscribed（实测）。
        let collected = json!({
            "resourceState": true,
            "album": { "id": 35088097, "name": "Cytus - Chapter K" },
            "songs": []
        });
        let detail = parse_album_detail(&collected).expect("已收藏专辑应可解析");
        assert!(detail.subscribed, "顶层 resourceState=true 应解析为已收藏");

        // 未收藏：该字段整体缺失。
        let plain = json!({ "album": { "id": 18909, "name": "Partners 拍档" }, "songs": [] });
        let detail = parse_album_detail(&plain).expect("未收藏专辑应可解析");
        assert!(!detail.subscribed, "缺少 resourceState 应视为未收藏");
    }

    #[test]
    fn parses_personalized_playlists() {
        let value = json!({
            "result": [
                { "id": 9, "name": "推荐", "picUrl": "http://x", "trackCount": 30, "playCount": 999 }
            ]
        });
        let list = parse_personalized_playlists(&value);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, 9);
        assert_eq!(list[0].track_count, 30);
    }

    #[test]
    fn parses_playlist_detail() {
        let value = json!({
            "playlist": {
                "id": 9, "name": "推荐", "coverImgUrl": "http://c",
                "tracks": [ { "id": 1, "name": "s", "ar": [{"name": "a"}], "al": {"name": "b"}, "dt": 1000 } ]
            }
        });
        let detail = parse_playlist_detail(&value).unwrap();
        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(detail.tracks[0].artists, "a");
    }

    #[test]
    fn parses_search_albums_artists_playlists() {
        let value = json!({
            "result": {
                "albums": [ { "id": 1, "name": "专", "picUrl": "http://a", "artists": [{"name": "甲"}] } ],
                "artists": [ { "id": 2, "name": "乙", "img1v1Url": "http://b" } ],
                "playlists": [ { "id": 3, "name": "单", "coverImgUrl": "http://c", "trackCount": 5 } ]
            }
        });
        let albums = parse_search_albums(&value);
        assert_eq!(albums[0].cover_url, "http://a");
        assert_eq!(albums[0].artists, "甲");
        let artists = parse_search_artists(&value);
        assert_eq!(artists[0].cover_url, "http://b");
        let playlists = parse_search_playlists(&value);
        assert_eq!(playlists[0].id, 3);
        assert_eq!(playlists[0].track_count, 5);
    }

    #[test]
    fn parses_user_level_and_bindings() {
        let level = parse_user_level(&json!({
            "code": 200,
            "data": { "level": 8, "nowPlayCount": 745, "nextPlayCount": 5000, "progress": 0.15 }
        }))
        .unwrap();
        assert_eq!(level.level, 8);
        assert_eq!(level.now_play_count, 745);

        let bindings = parse_bindings(&json!({
            "code": 200,
            "bindings": [ { "type": 1, "typeName": "手机", "expired": false } ]
        }));
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].kind, 1);
        assert_eq!(bindings[0].type_name, "手机");
    }

    #[test]
    fn parses_vip_info_active_and_expired() {
        // 黑胶 VIP 未过期 → vip_type 11，到期时间取 associator。
        let vip = parse_vip_info(&json!({
            "code": 200,
            "data": {
                "now": 1000,
                "redVipLevel": 7,
                "associator": { "expireTime": 5000 },
                "musicPackage": { "expireTime": 2000 }
            }
        }))
        .unwrap();
        assert_eq!(vip.vip_type, 11);
        assert_eq!(vip.red_vip_level, 7);
        assert_eq!(vip.expire_time, 5000);
        assert!(vip.is_vip);

        // 黑胶过期、音乐包有效 → 10。
        let vip = parse_vip_info(&json!({
            "code": 200,
            "data": {
                "now": 9000,
                "associator": { "expireTime": 5000 },
                "musicPackage": { "expireTime": 12000 }
            }
        }))
        .unwrap();
        assert_eq!(vip.vip_type, 10);
        assert_eq!(vip.expire_time, 12000);
    }
}
