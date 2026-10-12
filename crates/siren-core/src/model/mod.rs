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
}
