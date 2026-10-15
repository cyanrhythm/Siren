//! 业务 API：搜索、音源、账号。当前落地播放/登录链路所需子集。

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{parse_search_songs, Album, AlbumDetail, Artist, Playlist, PlaylistDetail, Song};
use crate::network::client::NeteaseClient;
use crate::network::endpoints::path;

/// 校验响应业务码：`301` → 未登录，其余非 `200`（且含 `code`）→ 业务错误。
///
/// 对齐 Melodia `ApiFlow`：成功码 `200`；部分接口省略 `code` 视为成功。
fn validate(value: &Value) -> Result<()> {
    match value.get("code").and_then(Value::as_i64) {
        None | Some(200) => Ok(()),
        Some(code) => {
            let message = value.get("message").and_then(Value::as_str).unwrap_or_default();
            Err(Error::from_code(code, message))
        }
    }
}

/// 综合搜索（单曲），`kind`：1=单曲 10=专辑 100=歌手 1000=歌单。
pub async fn search(client: &NeteaseClient, keyword: &str, kind: u32, limit: u32) -> Result<Vec<Song>> {
    let value = search_raw(client, keyword, kind, limit).await?;
    Ok(parse_search_songs(&value))
}

/// 原始搜索返回体。
pub async fn search_raw(client: &NeteaseClient, keyword: &str, kind: u32, limit: u32) -> Result<Value> {
    let body = json!({ "s": keyword, "type": kind, "limit": limit, "offset": 0 });
    let value = client.weapi_post(path::SEARCH, &body).await?;
    validate(&value)?;
    Ok(value)
}

/// 单曲播放地址（WEAPI，标准音质，匿名可用）。
pub async fn song_url(client: &NeteaseClient, id: i64) -> Result<Option<String>> {
    let body = json!({ "ids": format!("[{id}]"), "level": "standard", "encodeType": "aac" });
    let value = client.weapi_post(path::SONG_URL, &body).await?;
    validate(&value)?;
    Ok(extract_url(&value))
}

/// 单曲播放地址（EAPI，可指定音质；登录后解锁更高音质）。
///
/// `level`：`standard` / `higher` / `exhigh` / `lossless` / `hires`。
pub async fn song_url_eapi(client: &NeteaseClient, id: i64, level: &str) -> Result<Option<String>> {
    let encode = if matches!(level, "lossless" | "hires") {
        "flac"
    } else {
        "aac"
    };
    let body = json!({ "ids": format!("[{id}]"), "level": level, "encodeType": encode });
    let value = client.eapi_post(path::SONG_URL_EAPI, &body).await?;
    validate(&value)?;
    Ok(extract_url(&value))
}

/// 账号信息（含登录态）。
pub async fn account(client: &NeteaseClient) -> Result<Value> {
    let value = client.weapi_post(path::ACCOUNT, &json!({})).await?;
    validate(&value)?;
    Ok(value)
}

/// 推荐歌单。
pub async fn personalized_playlists(client: &NeteaseClient, limit: u32) -> Result<Vec<Playlist>> {
    let body = json!({ "limit": limit, "offset": 0, "total": true });
    let value = client.weapi_post(path::PERSONALIZED_PLAYLISTS, &body).await?;
    validate(&value)?;
    Ok(crate::model::parse_personalized_playlists(&value))
}

/// 歌单详情（含曲目）。
pub async fn playlist_detail(client: &NeteaseClient, id: i64) -> Result<Option<PlaylistDetail>> {
    let body = json!({ "id": id, "n": 1000, "s": 8 });
    let value = client.weapi_post(path::PLAYLIST_DETAIL, &body).await?;
    validate(&value)?;
    Ok(crate::model::parse_playlist_detail(&value))
}

/// 用户歌单（需登录）。
pub async fn user_playlists(client: &NeteaseClient, uid: i64, limit: u32, offset: u32) -> Result<Vec<Playlist>> {
    let body = json!({ "uid": uid, "limit": limit, "offset": offset });
    let value = client.weapi_post(path::USER_PLAYLIST, &body).await?;
    validate(&value)?;
    Ok(crate::model::parse_user_playlists(&value))
}

/// 从账号返回体取 uid。
pub fn account_uid(value: &Value) -> Option<i64> {
    value
        .get("profile")
        .and_then(|p| p.get("userId"))
        .and_then(Value::as_i64)
}

/// 从账号返回体取昵称。
pub fn account_nickname(value: &Value) -> Option<String> {
    value
        .get("profile")
        .and_then(|p| p.get("nickname"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
}

/// 从账号返回体取头像地址。
pub fn account_avatar(value: &Value) -> Option<String> {
    value
        .get("profile")
        .and_then(|p| p.get("avatarUrl"))
        .and_then(|a| a.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 喜欢 / 取消喜欢单曲（EAPI，需登录）。
pub async fn like_song(client: &NeteaseClient, id: i64, like: bool) -> Result<()> {
    let body = json!({ "trackId": id, "like": like });
    let value = client.eapi_post(path::SONG_LIKE, &body).await?;
    validate(&value)?;
    Ok(())
}

/// 当前用户已红心的歌曲 ID 列表（EAPI，需登录）。
pub async fn liked_song_ids(client: &NeteaseClient, uid: i64) -> Result<Vec<i64>> {
    let body = json!({ "uid": uid });
    let value = client.eapi_post(path::SONG_LIKE_GET, &body).await?;
    validate(&value)?;
    Ok(parse_liked_ids(&value))
}

/// 从红心列表返回体取 ID 数组。
pub fn parse_liked_ids(value: &Value) -> Vec<i64> {
    value
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

/// 云搜索（EAPI，按 `kind`：1 单曲 / 10 专辑 / 100 歌手 / 1000 歌单）。
pub async fn cloud_search(client: &NeteaseClient, keyword: &str, kind: u32, limit: u32) -> Result<Value> {
    let body = json!({ "s": keyword, "type": kind, "limit": limit, "offset": 0 });
    let value = client.eapi_post(path::CLOUD_SEARCH, &body).await?;
    validate(&value)?;
    Ok(value)
}

/// 最近播放歌曲（WEAPI，需登录）。
pub async fn recent_songs(client: &NeteaseClient) -> Result<Vec<Song>> {
    let value = client.weapi_post(path::RECENT_SONGS, &json!({ "limit": 100 })).await?;
    validate(&value)?;
    Ok(parse_recent_songs(&value))
}

/// 最近播放歌曲（含播放时间）。
pub async fn recent_songs_with_time(client: &NeteaseClient) -> Result<Vec<crate::model::RecentSong>> {
    let value = client.weapi_post(path::RECENT_SONGS, &json!({ "limit": 100 })).await?;
    validate(&value)?;
    Ok(crate::model::parse_recent_songs_with_time(&value))
}

/// 最近播放歌单（EAPI，需登录）。
pub async fn recent_playlists(client: &NeteaseClient) -> Result<Vec<crate::model::RecentPlaylist>> {
    let value = client
        .eapi_post(path::RECENT_PLAYLISTS, &json!({ "limit": 100 }))
        .await?;
    validate(&value)?;
    Ok(crate::model::parse_recent_playlists(&value))
}

/// 最近播放专辑（WEAPI，需登录）。
pub async fn recent_albums(client: &NeteaseClient) -> Result<Vec<crate::model::RecentAlbum>> {
    let value = client.weapi_post(path::RECENT_ALBUMS, &json!({ "limit": 100 })).await?;
    validate(&value)?;
    Ok(crate::model::parse_recent_albums(&value))
}

/// 从最近播放返回体取歌曲（`data.list[].data`）。
pub fn parse_recent_songs(value: &Value) -> Vec<Song> {
    value
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("data"))
                .filter(|data| data.get("id").is_some())
                .map(crate::model::parse_song)
                .collect()
        })
        .unwrap_or_default()
}

/// 批量歌曲详情（EAPI）：`ids` 一次不宜过多，调用方应分批。
pub async fn song_details(client: &NeteaseClient, ids: &[i64]) -> Result<Vec<Song>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let c = format!(
        "[{}]",
        ids.iter()
            .map(|id| format!("{{\"id\":{id}}}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let value = client.eapi_post(path::SONG_DETAIL_V3, &json!({ "c": c })).await?;
    validate(&value)?;
    Ok(value
        .get("songs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(crate::model::parse_song).collect())
        .unwrap_or_default())
}

/// 歌曲详情（EAPI `/eapi/v3/song/detail`）。
pub async fn song_detail(client: &NeteaseClient, id: i64) -> Result<Option<Song>> {
    let c = format!("[{{\"id\":{id}}}]");
    let value = client.eapi_post(path::SONG_DETAIL, &json!({ "c": c })).await?;
    validate(&value)?;
    Ok(value
        .get("songs")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .map(crate::model::parse_song))
}

/// 专辑详情（WEAPI，`/weapi/v1/album/{id}`）。
pub async fn album_detail(client: &NeteaseClient, id: i64) -> Result<Option<AlbumDetail>> {
    let url = format!("{}{id}", path::ALBUM_DETAIL);
    let value = client.weapi_post(&url, &json!({})).await?;
    validate(&value)?;
    Ok(crate::model::parse_album_detail(&value))
}

/// 收藏 / 取消收藏歌单（EAPI）。
pub async fn playlist_subscribe(client: &NeteaseClient, id: i64, subscribe: bool) -> Result<()> {
    let op = if subscribe { "subscribe" } else { "unsubscribe" };
    let url = format!("{}{op}", path::PLAYLIST_SUB);
    let value = client.eapi_post(&url, &json!({ "id": id })).await?;
    validate(&value)?;
    Ok(())
}

/// 收藏 / 取消收藏专辑（WEAPI）。
pub async fn album_subscribe(client: &NeteaseClient, id: i64, subscribe: bool) -> Result<()> {
    let op = if subscribe { "sub" } else { "unsub" };
    let url = format!("{}{op}", path::ALBUM_SUB);
    let value = client.weapi_post(&url, &json!({ "id": id })).await?;
    validate(&value)?;
    Ok(())
}

/// 歌单添加歌曲（EAPI）。
pub async fn playlist_add_tracks(client: &NeteaseClient, pid: i64, track_ids: &[i64]) -> Result<()> {
    manipulate_tracks(client, "add", pid, track_ids).await
}

/// 歌单移除歌曲（EAPI）。
pub async fn playlist_delete_tracks(client: &NeteaseClient, pid: i64, track_ids: &[i64]) -> Result<()> {
    manipulate_tracks(client, "del", pid, track_ids).await
}

async fn manipulate_tracks(client: &NeteaseClient, op: &str, pid: i64, track_ids: &[i64]) -> Result<()> {
    let ids = track_ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let body = json!({ "op": op, "pid": pid, "trackIds": format!("[{ids}]"), "imme": "true" });
    let value = client.eapi_post(path::PLAYLIST_MANIPULATE, &body).await?;
    validate(&value)?;
    Ok(())
}

/// 歌单改名（EAPI）。
pub async fn playlist_rename(client: &NeteaseClient, id: i64, name: &str) -> Result<()> {
    let value = client
        .eapi_post(path::PLAYLIST_UPDATE_NAME, &json!({ "id": id, "name": name }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 删除歌单（WEAPI）。
pub async fn playlist_delete(client: &NeteaseClient, id: i64) -> Result<()> {
    let value = client
        .weapi_post(path::PLAYLIST_REMOVE, &json!({ "ids": format!("[{id}]") }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 更换歌单封面：图片字节经 NOS 直传后登记，返回服务端下发的封面地址。
///
/// 流程对齐 Melodia `UpdatePlaylistCoverUseCase` + `NosUploadClient`：
/// 申请 token → 取上传主机 → 传字节 → 登记（`coverImgId` = `docId`）。
pub async fn update_playlist_cover(
    client: &NeteaseClient,
    playlist_id: i64,
    bytes: Vec<u8>,
    ext: &str,
    filename: &str,
) -> Result<String> {
    // 1. 申请上传凭证（封面固定 bucket=yyimgs、nos_product=0、type=other，不带 md5）
    let token_value = client
        .weapi_post(
            path::NOS_TOKEN_ALLOC,
            &json!({
                "bucket": "yyimgs",
                "ext": ext,
                "filename": filename,
                "nos_product": 0,
                "type": "other",
                "return_body": "{\"code\":200,\"size\":\"$(ObjectSize)\"}"
            }),
        )
        .await?;
    validate(&token_value)?;
    let result = token_value.get("result").unwrap_or(&serde_json::Value::Null);
    let bucket = result
        .get("bucket")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let nos_token = result
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let object_key = result
        .get("objectKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let doc_id = result
        .get("docId")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or_default();
    if bucket.is_empty() || object_key.is_empty() || nos_token.is_empty() || doc_id <= 0 {
        return Err(Error::Biz {
            code: 0,
            message: "NOS 凭证获取失败".into(),
        });
    }

    // 2. 取上传主机（第三方主机，使用独立 client，不带 Cookie）
    let http = reqwest::Client::new();
    let lbs: Value = http
        .get(format!("https://wanproxy.127.net/lbs?version=1.0&bucketname={bucket}"))
        .send()
        .await?
        .json()
        .await?;
    let host = lbs
        .get("upload")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Network("NOS 上传地址获取失败".into()))?
        .to_string();

    // 3. 上传字节（objectKey 含斜杠需转义为单一路径段）
    let encoded = object_key.replace('/', "%2F");
    let url = format!("{host}/{bucket}/{encoded}?offset=0&complete=true&version=1.0");
    let mime = if ext.eq_ignore_ascii_case("png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    let resp = http
        .post(url)
        .header("x-nos-token", nos_token)
        .header("Content-Type", mime)
        .body(bytes)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(Error::Network(format!("NOS 上传失败: HTTP {}", resp.status())));
    }

    // 4. 登记封面（coverImgId = docId），返回服务端下发的封面地址
    let value = client
        .weapi_post(
            path::PLAYLIST_COVER_UPDATE,
            &json!({ "id": playlist_id, "coverImgId": doc_id }),
        )
        .await?;
    validate(&value)?;
    Ok(value.get("url").and_then(Value::as_str).unwrap_or_default().to_string())
}

/// 更新歌单简介（EAPI）。
pub async fn playlist_update_desc(client: &NeteaseClient, id: i64, desc: &str) -> Result<()> {
    let value = client
        .eapi_post(path::PLAYLIST_UPDATE_DESC, &json!({ "id": id, "desc": desc }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 新建歌单（EAPI），返回新歌单 id。
pub async fn playlist_create(client: &NeteaseClient, name: &str, privacy: i64) -> Result<i64> {
    let body = json!({ "name": name, "privacy": privacy, "type": "NORMAL" });
    let value = client.eapi_post(path::PLAYLIST_CREATE, &body).await?;
    validate(&value)?;
    let id = value
        .get("id")
        .or_else(|| value.get("playlist").and_then(|p| p.get("id")))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    // 接口成功但未返回 id：视为失败，避免上层拿到 0 当作有效歌单。
    if id <= 0 {
        return Err(Error::Biz {
            code: 0,
            message: "新建歌单失败：接口未返回歌单 id".into(),
        });
    }
    Ok(id)
}

/// 歌曲百科 + 制作人（WEAPI）。
pub async fn song_wiki(client: &NeteaseClient, id: i64) -> Result<crate::model::SongWiki> {
    let wiki = client.weapi_post(path::SONG_WIKI, &json!({ "songId": id })).await?;
    validate(&wiki)?;
    let creators = client.weapi_post(path::SONG_CREATORS, &json!({ "songId": id })).await?;
    validate(&creators)?;
    Ok(crate::model::parse_song_wiki(&wiki, &creators))
}

/// 歌曲评论的 threadId。
pub fn comment_thread_id(song_id: i64) -> String {
    format!("R_SO_4_{song_id}")
}

/// 评论列表（按 `cursor` 翻页；`0` 为最新）。
pub async fn comments(
    client: &NeteaseClient,
    thread_id: &str,
    cursor: &str,
    page_size: u32,
) -> Result<crate::model::CommentPage> {
    let body = json!({
        "threadId": thread_id,
        "pageNo": 1,
        "pageSize": page_size,
        "cursor": cursor,
        "sortType": 2,
        "showInner": true
    });
    let value = client.weapi_post(path::COMMENTS_V2, &body).await?;
    validate(&value)?;
    Ok(crate::model::parse_comments_v2(&value))
}

/// 评论点赞 / 取消。
pub async fn comment_like(client: &NeteaseClient, thread_id: &str, comment_id: i64, like: bool) -> Result<()> {
    let op = if like { "like" } else { "unlike" };
    let url = format!("{}{op}", path::COMMENT_LIKE);
    let value = client
        .weapi_post(&url, &json!({ "threadId": thread_id, "commentId": comment_id }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 评论楼层（子回复）。
pub async fn comment_floor(
    client: &NeteaseClient,
    thread_id: &str,
    parent_id: i64,
    limit: u32,
) -> Result<crate::model::CommentPage> {
    let body = json!({ "parentCommentId": parent_id, "threadId": thread_id, "time": -1, "limit": limit });
    let value = client.weapi_post(path::COMMENT_FLOOR, &body).await?;
    validate(&value)?;
    let data = value.get("data").cloned().unwrap_or(serde_json::Value::Null);
    Ok(crate::model::parse_comments_v2(&json!({ "data": data })))
}

/// 首页货架（EAPI）。
pub async fn home_shelves(client: &NeteaseClient) -> Result<Vec<crate::model::HomeShelf>> {
    let value = client.eapi_post(path::HOME_BLOCK_PAGE, &json!({})).await?;
    validate(&value)?;
    Ok(crate::model::parse_home_shelves(&value))
}

/// 每日推荐歌曲（EAPI）。
pub async fn daily_recommend_songs(client: &NeteaseClient) -> Result<Vec<Song>> {
    let value = client.eapi_post(path::DAILY_RECOMMEND, &json!({})).await?;
    validate(&value)?;
    Ok(value
        .get("data")
        .and_then(|d| d.get("dailySongs"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(crate::model::parse_song).collect())
        .unwrap_or_default())
}

/// 用户详情（WEAPI）。
pub async fn user_detail(client: &NeteaseClient, uid: i64) -> Result<Option<crate::model::UserProfile>> {
    let url = format!("{}{uid}", path::USER_DETAIL);
    let value = client.weapi_post(&url, &json!({})).await?;
    validate(&value)?;
    Ok(crate::model::parse_user_profile(&value))
}

/// 用户等级（EAPI）。
pub async fn user_level(client: &NeteaseClient) -> Result<crate::model::UserLevel> {
    let value = client.eapi_post(path::USER_LEVEL, &json!({})).await?;
    validate(&value)?;
    Ok(crate::model::parse_user_level(&value).unwrap_or_default())
}

/// VIP 状态信息（WEAPI）。
pub async fn vip_info(client: &NeteaseClient) -> Result<crate::model::VipInfo> {
    let value = client.weapi_post(path::VIP_INFO, &json!({})).await?;
    validate(&value)?;
    Ok(crate::model::parse_vip_info(&value).unwrap_or_default())
}

/// 账号第三方绑定列表（WEAPI；该接口可能不可用，失败由调用方降级）。
pub async fn user_bindings(client: &NeteaseClient, uid: i64) -> Result<Vec<crate::model::AccountBinding>> {
    let value = client.weapi_post(path::USER_BINDING, &json!({ "uid": uid })).await?;
    validate(&value)?;
    Ok(crate::model::parse_bindings(&value))
}

/// 修改用户资料（EAPI，目前仅昵称与个性签名）。
pub async fn update_user_profile(client: &NeteaseClient, nickname: &str, signature: &str) -> Result<()> {
    let value = client
        .eapi_post(
            path::USER_UPDATE,
            &json!({
                "nickname": nickname,
                "gender": 1,
                "birthday": 0,
                "province": 110000,
                "city": 110101,
                "signature": signature,
            }),
        )
        .await?;
    validate(&value)?;
    Ok(())
}

/// 昵称查重（EAPI），返回是否已被占用。
pub async fn check_nickname(client: &NeteaseClient, nickname: &str) -> Result<bool> {
    let value = client
        .eapi_post(path::NICKNAME_CHECK, &json!({ "nickname": nickname }))
        .await?;
    // 校验业务码：否则接口报错（如未登录）会被当成「未被占用」。
    validate(&value)?;
    Ok(value.get("duplicated").and_then(Value::as_bool).unwrap_or(false))
}

/// 每日签到（EAPI），返回获得的积分；重复签到（`code=-2`）返回 0。
pub async fn daily_signin(client: &NeteaseClient, kind: i64) -> Result<i64> {
    let value = client.eapi_post(path::DAILY_TASK, &json!({ "type": kind })).await?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(200);
    if code != 200 && code != -2 {
        let message = value.get("msg").and_then(Value::as_str).unwrap_or_default();
        return Err(Error::from_code(code, message));
    }
    Ok(value.get("point").and_then(Value::as_i64).unwrap_or(0))
}

/// 更换头像（WEAPI multipart），返回新头像地址。
pub async fn upload_avatar(client: &NeteaseClient, bytes: Vec<u8>, filename: &str) -> Result<String> {
    let value = client
        .weapi_multipart(path::AVATAR_UPLOAD, "imgFile", filename, "image/jpeg", bytes)
        .await?;
    validate(&value)?;
    Ok(value
        .get("data")
        .and_then(|d| d.get("avatarUrl"))
        .or_else(|| value.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// 关注 / 取消关注用户。
pub async fn user_follow(client: &NeteaseClient, uid: i64, follow: bool) -> Result<()> {
    let url = if follow {
        format!("{}{uid}", path::USER_FOLLOW)
    } else {
        format!("{}{uid}", path::USER_DELFOLLOW)
    };
    let value = client.weapi_post(&url, &json!({})).await?;
    validate(&value)?;
    Ok(())
}

/// 用户关注列表。
pub async fn user_follows(client: &NeteaseClient, uid: i64, limit: u32) -> Result<Vec<crate::model::UserBrief>> {
    let url = format!("{}{uid}", path::USER_FOLLOWS);
    let value = client
        .weapi_post(&url, &json!({ "offset": 0, "limit": limit, "order": true }))
        .await?;
    validate(&value)?;
    Ok(crate::model::parse_user_follow_list(&value))
}

/// 用户粉丝列表。
pub async fn user_followeds(client: &NeteaseClient, uid: i64, limit: u32) -> Result<Vec<crate::model::UserBrief>> {
    let url = format!("{}{uid}", path::USER_FOLLOWEDS);
    let value = client
        .weapi_post(
            &url,
            &json!({ "userId": uid, "time": "0", "limit": limit, "offset": 0, "getcounts": "true" }),
        )
        .await?;
    validate(&value)?;
    Ok(crate::model::parse_user_follow_list(&value))
}

/// 更新用户歌单排序（`ids` 为完整顺序，逗号分隔）。
pub async fn playlist_order_update(client: &NeteaseClient, ids: &[i64]) -> Result<()> {
    let joined = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let value = client
        .weapi_post(path::PLAYLIST_ORDER, &json!({ "ids": joined }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 置顶 / 取消置顶自建歌单。
pub async fn playlist_top(client: &NeteaseClient, id: i64, top: bool) -> Result<()> {
    let url = if top {
        path::PLAYLIST_TOP.to_string()
    } else {
        format!("{}/cancel", path::PLAYLIST_TOP)
    };
    let value = client.weapi_post(&url, &json!({ "id": id })).await?;
    validate(&value)?;
    Ok(())
}

/// 收藏专辑列表（EAPI）。
pub async fn album_sublist(client: &NeteaseClient) -> Result<Vec<Album>> {
    let value = client
        .eapi_post(
            path::ALBUM_SUBLIST,
            &json!({ "limit": 1000, "offset": 0, "total": true }),
        )
        .await?;
    validate(&value)?;
    Ok(crate::model::parse_album_sublist(&value))
}

/// 收藏歌手列表（EAPI，显式分页取全；服务端默认仅返回 25 条）。
pub async fn artist_sublist(client: &NeteaseClient) -> Result<Vec<Artist>> {
    const PAGE: u32 = 100;
    let mut out = Vec::new();
    let mut offset = 0u32;
    loop {
        let value = client
            .eapi_post(
                path::ARTIST_SUBLIST,
                &json!({ "limit": PAGE, "offset": offset, "total": true }),
            )
            .await?;
        validate(&value)?;
        let page = crate::model::parse_artist_sublist(&value);
        let got = page.len();
        out.extend(page);
        if got < PAGE as usize || out.len() >= 2000 {
            break;
        }
        offset += PAGE;
    }
    Ok(out)
}

/// 歌手热门歌曲（WEAPI）。
pub async fn artist_top_songs(client: &NeteaseClient, id: i64) -> Result<Vec<Song>> {
    let value = client.weapi_post(path::ARTIST_TOP_SONGS, &json!({ "id": id })).await?;
    validate(&value)?;
    Ok(crate::model::parse_artist_top_songs(&value))
}

/// 歌手详情（EAPI）。
pub async fn artist_detail(client: &NeteaseClient, id: i64) -> Result<Option<crate::model::ArtistDetail>> {
    let value = client.eapi_post(path::ARTIST_DETAIL, &json!({ "id": id })).await?;
    validate(&value)?;
    Ok(crate::model::parse_artist_detail(&value))
}

/// 歌手专辑（WEAPI）。
pub async fn artist_albums(client: &NeteaseClient, id: i64, limit: u32) -> Result<Vec<crate::model::ArtistAlbum>> {
    let url = format!("{}{id}", path::ARTIST_ALBUMS);
    let value = client
        .weapi_post(&url, &json!({ "limit": limit, "offset": 0, "total": true }))
        .await?;
    validate(&value)?;
    Ok(crate::model::parse_artist_albums(&value))
}

/// 相似歌手（EAPI）。
pub async fn similar_artists(client: &NeteaseClient, id: i64) -> Result<Vec<crate::model::Artist>> {
    let value = client.eapi_post(path::ARTIST_SIMI, &json!({ "artistid": id })).await?;
    validate(&value)?;
    Ok(crate::model::parse_simi_artists(&value))
}

/// 歌手关注数 / 是否已关注（WEAPI）→ `(粉丝数, 是否关注)`。
pub async fn artist_follow_count(client: &NeteaseClient, id: i64) -> Result<(i64, bool)> {
    let value = client
        .weapi_post(path::ARTIST_FOLLOW_COUNT, &json!({ "id": id }))
        .await?;
    validate(&value)?;
    let data = value.get("data").unwrap_or(&serde_json::Value::Null);
    let fans = data
        .get("fansCnt")
        .or_else(|| data.get("fansCount"))
        .or_else(|| data.get("fans"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let followed = data
        .get("isFollow")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok((fans, followed))
}

/// 相似歌曲（EAPI）。
pub async fn simi_songs(client: &NeteaseClient, id: i64) -> Result<Vec<Song>> {
    let value = client.eapi_post(path::SIMI_SONG, &json!({ "songid": id })).await?;
    validate(&value)?;
    Ok(value
        .get("songs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(crate::model::parse_song).collect())
        .unwrap_or_default())
}

/// 心动模式推荐（EAPI）：按 `song_id` / `playlist_id` 拉取推荐队列。
pub async fn intelligence_songs(
    client: &NeteaseClient,
    song_id: i64,
    playlist_id: i64,
    kind: &str,
) -> Result<Vec<Song>> {
    let body = json!({
        "songId": song_id,
        "playlistId": playlist_id,
        "type": kind,
        "startMusicId": song_id,
        "count": 20
    });
    let value = client.eapi_post(path::INTELLIGENCE, &body).await?;
    validate(&value)?;
    Ok(value
        .get("data")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("songInfo"))
                .filter(|s| s.get("id").is_some())
                .map(crate::model::parse_song)
                .collect()
        })
        .unwrap_or_default())
}

/// 收藏 / 取消收藏歌手（WEAPI）。
pub async fn artist_follow(client: &NeteaseClient, id: i64, follow: bool) -> Result<()> {
    let op = if follow { "sub" } else { "unsub" };
    let url = format!("{}{op}", path::ARTIST_SUB);
    let value = client
        .weapi_post(&url, &json!({ "artistId": id, "artistIds": format!("[{id}]") }))
        .await?;
    validate(&value)?;
    Ok(())
}

/// 刷新登录态：新 Cookie 经响应头 `Set-Cookie` 下发，成功返回合并后的 Cookie。
pub async fn refresh_login(client: &NeteaseClient) -> Result<Option<String>> {
    let (value, cookie) = client.weapi_post_with_cookie(path::LOGIN_REFRESH, &json!({})).await?;
    validate(&value)?;
    let Some(refresh) = cookie.filter(|c| c.contains("MUSIC_U=")) else {
        return Ok(None);
    };
    // 按键合并：服务端可能只重发 MUSIC_U，需保留旧 __csrf 等。
    let merged = crate::network::client::merge_cookies(client.cookie().as_deref(), &refresh);
    Ok(Some(merged))
}

/// 歌词（EAPI `/eapi/song/lyric/v1`）：优先逐字（yrc），合并译文（tlyric）。
pub async fn lyrics(client: &NeteaseClient, id: i64) -> Result<Vec<crate::lyrics::LyricLine>> {
    let body = json!({
        "id": id,
        "cp": false,
        "tv": -1,
        "lv": -1,
        "rv": -1,
        "kv": -1,
        "yv": 99,
        "ytv": -1,
        "yrv": -1
    });
    let value = client.eapi_post(path::LYRIC, &body).await?;
    validate(&value)?;
    Ok(parse_lyrics(&value))
}

/// 从歌词返回体解析歌词行（优先 yrc，其次 lrc，并合并 tlyric 译文）。
pub fn parse_lyrics(value: &Value) -> Vec<crate::lyrics::LyricLine> {
    let content = |key: &str| -> &str {
        value
            .get(key)
            .and_then(|c| c.get("lyric"))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    let yrc = content("yrc");
    let mut lines = if yrc.trim().is_empty() {
        crate::lyrics::parse(content("lrc"))
    } else {
        crate::lyrics::parse(yrc)
    };
    let tlyric = content("tlyric");
    if !tlyric.trim().is_empty() {
        let t = crate::lyrics::parse(tlyric);
        crate::lyrics::merge_translation(&mut lines, &t);
    }
    lines
}

/// 打卡上报：开始播放（`action=startplay`），使歌曲进入「最近播放」。
pub async fn report_start_play(client: &NeteaseClient, song_id: i64, container_id: i64) -> Result<()> {
    let logs = json!([
        {
            "action": "startplay",
            "json": {
                "id": song_id,
                "type": "song",
                "mainsite": "1",
                "mainsiteWeb": "1",
                "content": format!("id={container_id}")
            }
        }
    ])
    .to_string();
    let value = client.eabi_post_weblog(path::WEBLOG, &json!({ "logs": logs })).await?;
    validate(&value)?;
    Ok(())
}

/// 打卡上报：播放结束（`action=play`，`time` 为实际播放秒数），涨「听歌排行」计数。
pub async fn report_play_end(client: &NeteaseClient, song_id: i64, container_id: i64, seconds: i64) -> Result<()> {
    let logs = json!([
        {
            "action": "play",
            "json": {
                "download": 0,
                "end": "playend",
                "id": song_id,
                "sourceId": container_id,
                "time": seconds,
                "type": "song",
                "wifi": 0,
                "source": "list",
                "mainsite": "1",
                "mainsiteWeb": "1",
                "content": format!("id={container_id}")
            }
        }
    ])
    .to_string();
    let value = client.eabi_post_weblog(path::WEBLOG, &json!({ "logs": logs })).await?;
    validate(&value)?;
    Ok(())
}

/// 二维码登录：申请 unikey。
pub async fn login_qr_key(client: &NeteaseClient) -> Result<String> {
    let value = client.weapi_post(path::QR_UNIKEY, &json!({ "type": 1 })).await?;
    validate(&value)?;
    Ok(value
        .get("unikey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// 二维码内容（供生成二维码图片）。
pub fn qr_login_url(key: &str) -> String {
    format!("https://music.163.com/login?codekey={key}")
}

/// 二维码扫码状态：`(code, set_cookie, message)`。
///
/// `code`：800 过期 / 801 待扫 / 802 待确认 / 803 成功（Cookie 在响应头）。
pub async fn login_qr_check(client: &NeteaseClient, key: &str) -> Result<(i64, Option<String>, String)> {
    let (value, cookie) = client
        .weapi_post_with_cookie(path::QR_CHECK, &json!({ "key": key, "type": 1 }))
        .await?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or_default();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((code, cookie, message))
}

/// 从播放地址返回体取第一条的 url。
fn extract_url(value: &Value) -> Option<String> {
    value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("url"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_liked_ids() {
        assert_eq!(parse_liked_ids(&json!({ "ids": [1, 2, 3] })), vec![1, 2, 3]);
    }

    #[test]
    fn missing_ids_is_empty() {
        assert!(parse_liked_ids(&json!({})).is_empty());
    }

    #[test]
    fn validate_flags_301() {
        assert!(validate(&json!({ "code": 301 })).unwrap_err().is_unauthorized());
        assert!(validate(&json!({ "code": 200 })).is_ok());
        assert!(validate(&json!({})).is_ok());
    }

    #[test]
    fn parses_lyrics_prefers_yrc_and_merges_translation() {
        let value = json!({
            "code": 200,
            "yrc": { "lyric": "[0,1000](0,500,0)你(500,500,0)好" },
            "tlyric": { "lyric": "[00:00.00]hello" }
        });
        let lines = parse_lyrics(&value);
        assert_eq!(lines[0].text, "你好");
        assert_eq!(lines[0].translation.as_deref(), Some("hello"));
    }

    #[test]
    fn parses_recent_songs() {
        let value = json!({
            "code": 200,
            "data": { "list": [
                { "playTime": 1, "data": { "id": 7, "name": "s", "ar": [{"name": "a"}], "al": {"name": "b", "picUrl": "u"}, "dt": 1000 } }
            ]}
        });
        let songs = parse_recent_songs(&value);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, 7);
        assert_eq!(songs[0].cover_url, "u");
    }
}
