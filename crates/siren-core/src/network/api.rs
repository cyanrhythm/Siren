//! 业务 API：搜索、音源、账号。当前落地播放/登录链路所需子集。

use serde_json::{json, Value};

use crate::error::Result;
use crate::model::{parse_search_songs, Playlist, PlaylistDetail, Song};
use crate::network::client::NeteaseClient;
use crate::network::endpoints::path;

/// 综合搜索（单曲），`kind`：1=单曲 10=专辑 100=歌手 1000=歌单。
pub async fn search(client: &NeteaseClient, keyword: &str, kind: u32, limit: u32) -> Result<Vec<Song>> {
    let value = search_raw(client, keyword, kind, limit).await?;
    Ok(parse_search_songs(&value))
}

/// 原始搜索返回体。
pub async fn search_raw(client: &NeteaseClient, keyword: &str, kind: u32, limit: u32) -> Result<Value> {
    let body = json!({ "s": keyword, "type": kind, "limit": limit, "offset": 0 });
    client.weapi_post(path::SEARCH, &body).await
}

/// 单曲播放地址（WEAPI，标准音质，匿名可用）。
pub async fn song_url(client: &NeteaseClient, id: i64) -> Result<Option<String>> {
    let body = json!({ "ids": format!("[{id}]"), "level": "standard", "encodeType": "aac" });
    let value = client.weapi_post(path::SONG_URL, &body).await?;
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
    Ok(extract_url(&value))
}

/// 账号信息（含登录态）。
pub async fn account(client: &NeteaseClient) -> Result<Value> {
    client.weapi_post(path::ACCOUNT, &json!({})).await
}

/// 推荐歌单。
pub async fn personalized_playlists(client: &NeteaseClient, limit: u32) -> Result<Vec<Playlist>> {
    let body = json!({ "limit": limit, "offset": 0, "total": true });
    let value = client.weapi_post(path::PERSONALIZED_PLAYLISTS, &body).await?;
    Ok(crate::model::parse_personalized_playlists(&value))
}

/// 歌单详情（含曲目）。
pub async fn playlist_detail(client: &NeteaseClient, id: i64) -> Result<Option<PlaylistDetail>> {
    let body = json!({ "id": id, "n": 1000, "s": 8 });
    let value = client.weapi_post(path::PLAYLIST_DETAIL, &body).await?;
    Ok(crate::model::parse_playlist_detail(&value))
}

/// 用户歌单（需登录）。
pub async fn user_playlists(client: &NeteaseClient, uid: i64, limit: u32, offset: u32) -> Result<Vec<Playlist>> {
    let body = json!({ "uid": uid, "limit": limit, "offset": offset });
    let value = client.weapi_post(path::USER_PLAYLIST, &body).await?;
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
