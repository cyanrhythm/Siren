//! 业务 API：搜索、音源、账号。当前落地播放/登录链路所需子集。

use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::model::{parse_search_songs, Playlist, PlaylistDetail, Song};
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
