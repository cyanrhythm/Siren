//! 数据模型：API 返回结构与领域实体。
//!
//! 对应 Melodia 的 `core/model/`。当前落地播放/搜索所需的子集，
//! 后续按业务域（歌单、专辑、歌手、播客等）扩展。

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
    /// 时长（毫秒），源头可能缺失。
    pub duration_ms: i64,
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

/// 歌单详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub cover_url: String,
    pub tracks: Vec<Song>,
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
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        track_count: value.get("trackCount").and_then(Value::as_i64).unwrap_or_default(),
        play_count: value.get("playCount").and_then(Value::as_i64).unwrap_or_default(),
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
    Some(PlaylistDetail {
        id: playlist.get("id").and_then(Value::as_i64).unwrap_or_default(),
        name: str_field(playlist, "name"),
        cover_url: playlist
            .get("coverImgUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tracks,
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

fn parse_song(value: &Value) -> Song {
    let id = value.get("id").and_then(Value::as_i64).unwrap_or_default();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // 兼容经典 search/get（artists/album/duration）与 cloudsearch（ar/al/dt）
    let artists = value
        .get("artists")
        .or_else(|| value.get("ar"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();

    let album = value
        .get("album")
        .or_else(|| value.get("al"))
        .and_then(|al| al.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let duration_ms = value
        .get("duration")
        .or_else(|| value.get("dt"))
        .and_then(Value::as_i64)
        .unwrap_or_default();

    Song {
        id,
        name,
        artists,
        album,
        duration_ms,
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
                  "album": {"name": "专"}, "duration": 123000 }
            ]}
        });
        let songs = parse_search_songs(&value);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].artists, "甲/乙");
        assert_eq!(songs[0].album, "专");
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
}
