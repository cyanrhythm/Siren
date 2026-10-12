//! 歌曲百科与制作人信息。对应 Melodia `feature/player/domain/SongWikiData.kt`。

use serde_json::Value;

/// 制作人员单个角色（如「作词」）及其艺人名单。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CreatorRole {
    pub role: String,
    pub artists: Vec<String>,
}

/// 回忆坐标。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MusicMemory {
    pub first_listen_date: String,
    pub season: String,
    pub period: String,
    pub play_count: i64,
    pub play_count_text: String,
}

/// 歌曲百科/详情。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SongWiki {
    /// 曲风。
    pub style: String,
    /// 专辑名。
    pub album: String,
    /// 语种。
    pub language: String,
    /// 发行时间（字符串，源格式）。
    pub publish_time: String,
    /// BPM。
    pub bpm: String,
    /// 影综。
    pub entertainment: String,
    /// 获奖成就（可能少于总数）。
    pub awards: Vec<String>,
    /// 获奖总数。
    pub award_total: i64,
    /// 制作人员（按角色）。
    pub creator_roles: Vec<CreatorRole>,
    /// 回忆坐标。
    pub music_memory: Option<MusicMemory>,
}

impl SongWiki {
    /// 是否无任何可用信息。
    pub fn is_empty(&self) -> bool {
        self.style.is_empty()
            && self.album.is_empty()
            && self.language.is_empty()
            && self.publish_time.is_empty()
            && self.bpm.is_empty()
            && self.entertainment.is_empty()
            && self.awards.is_empty()
            && self.creator_roles.is_empty()
            && self.music_memory.is_none()
    }
}

/// 合并百科摘要（`blocks`）与制作人（`songCreatorsRoleVos`）两个返回体。
pub fn parse_song_wiki(wiki: &Value, creators: &Value) -> SongWiki {
    let mut out = SongWiki::default();

    if let Some(blocks) = wiki.get("data").and_then(|d| d.get("blocks")).and_then(Value::as_array) {
        for block in blocks {
            let code = block.get("code").and_then(Value::as_str).unwrap_or_default();
            if code == "SONG_PLAY_ABOUT_MUSIC_MEMORY" {
                if let Some(memory) = parse_music_memory(block) {
                    out.music_memory = Some(memory);
                }
                continue;
            }
            if code != "SONG_PLAY_ABOUT_SONG_BASIC" {
                continue;
            }
            let Some(creatives) = block.get("creatives").and_then(Value::as_array) else {
                continue;
            };
            for creative in creatives {
                let kind = creative.get("creativeType").and_then(Value::as_str).unwrap_or_default();
                let titles = |c: &Value| -> String {
                    c.get("resources")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|r| {
                                    r.get("uiElement")
                                        .and_then(|u| u.get("mainTitle"))
                                        .and_then(|m| m.get("title"))
                                        .and_then(Value::as_str)
                                })
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" / ")
                        })
                        .unwrap_or_default()
                };
                let first_text = |c: &Value| -> String {
                    c.get("uiElement")
                        .and_then(|u| u.get("textLinks"))
                        .and_then(Value::as_array)
                        .and_then(|arr| arr.first())
                        .and_then(|t| t.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                match kind {
                    "songTag" => out.style = titles(creative),
                    "language" => out.language = first_text(creative),
                    "bpm" => out.bpm = first_text(creative),
                    "entertainment" => out.entertainment = titles(creative),
                    "songAward" => {
                        out.awards = creative
                            .get("resources")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|r| {
                                        r.get("uiElement")
                                            .and_then(|u| u.get("mainTitle"))
                                            .and_then(|m| m.get("title"))
                                            .and_then(Value::as_str)
                                    })
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .collect()
                            })
                            .unwrap_or_default();
                        out.award_total = creative
                            .get("uiElement")
                            .and_then(|u| u.get("buttons"))
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.first())
                            .and_then(|b| b.get("text"))
                            .and_then(Value::as_str)
                            .and_then(|t| t.chars().filter(char::is_ascii_digit).collect::<String>().parse().ok())
                            .unwrap_or(0);
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(roles) = creators
        .get("data")
        .and_then(|d| d.get("songCreatorsRoleVos"))
        .and_then(Value::as_array)
    {
        out.creator_roles = roles
            .iter()
            .filter_map(|role| {
                let artists: Vec<String> = role
                    .get("creatorMetaVOS")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("artistName").and_then(Value::as_str))
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if artists.is_empty() {
                    None
                } else {
                    Some(CreatorRole {
                        role: role
                            .get("roleName")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        artists,
                    })
                }
            })
            .collect();
    }

    out
}

fn parse_music_memory(block: &Value) -> Option<MusicMemory> {
    let resources: Vec<Value> = block
        .get("creatives")
        .and_then(Value::as_array)
        .map(|creatives| {
            creatives
                .iter()
                .flat_map(|c| {
                    c.get("resources")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let first = resources
        .iter()
        .find(|r| r.get("resourceType").and_then(Value::as_str) == Some("FIRST_LISTEN"))
        .and_then(|r| r.get("resourceExt"))
        .and_then(|e| e.get("musicFirstListenDto"));
    let total = resources
        .iter()
        .find(|r| r.get("resourceType").and_then(Value::as_str) == Some("TOTAL_PLAY"))
        .and_then(|r| r.get("resourceExt"))
        .and_then(|e| e.get("musicTotalPlayDto"));

    let date = first
        .and_then(|d| d.get("date"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let play_count = total
        .and_then(|t| t.get("playCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if date.trim().is_empty() && play_count == 0 {
        return None;
    }
    Some(MusicMemory {
        first_listen_date: date,
        season: first
            .and_then(|d| d.get("season"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        period: first
            .and_then(|d| d.get("period"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        play_count,
        play_count_text: total
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_basic_block_and_creators() {
        let wiki = json!({
            "data": { "blocks": [
                { "code": "SONG_PLAY_ABOUT_SONG_BASIC", "creatives": [
                    { "creativeType": "songTag", "resources": [ { "uiElement": { "mainTitle": { "title": "流行" } } }, { "uiElement": { "mainTitle": { "title": "摇滚" } } } ] },
                    { "creativeType": "language", "uiElement": { "textLinks": [ { "text": "国语" } ] } },
                    { "creativeType": "bpm", "uiElement": { "textLinks": [ { "text": "128" } ] } }
                ] }
            ] }
        });
        let creators = json!({
            "data": { "songCreatorsRoleVos": [
                { "roleName": "作词", "creatorMetaVOS": [ { "artistName": "甲" }, { "artistName": "乙" } ] },
                { "roleName": "作曲", "creatorMetaVOS": [] }
            ] }
        });
        let w = parse_song_wiki(&wiki, &creators);
        assert_eq!(w.style, "流行 / 摇滚");
        assert_eq!(w.language, "国语");
        assert_eq!(w.bpm, "128");
        assert_eq!(w.creator_roles.len(), 1);
        assert_eq!(w.creator_roles[0].role, "作词");
        assert_eq!(w.creator_roles[0].artists, vec!["甲", "乙"]);
    }

    #[test]
    fn empty_inputs_are_empty() {
        assert!(parse_song_wiki(&json!({}), &json!({})).is_empty());
    }
}
