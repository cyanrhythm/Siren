//! 歌词解析与时间轴。
//!
//! 对应 Melodia `core/player/domain/` 的 `LyricParser` / `QrcLyricParser`。
//!
//! ## 支持格式
//!
//! | 格式 | 说明 |
//! |---|---|
//! | LRC | 逐行歌词（含一行多时间标签；增强型 `<mm:ss.xx>` 逐字） |
//! | YRC | 网易云逐字歌词 `[start,dur](start,dur,0)word` |
//! | QRC | QQ 逐字歌词 `[start,dur]word(start,dur)` |
//!
//! TTML（AMLL）暂未实现。

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// 单个字/词的耗时元数据。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WordInfo {
    pub text: String,
    /// 相对整行起始时间的偏移（毫秒）。
    pub start_offset_ms: i64,
    /// 该字/词的持续发音时长（毫秒）。
    pub duration_ms: i64,
}

/// 歌词行。
///
/// 可序列化：磁盘歌词缓存直接存结构化的行（解析结果已含译文合并与逐字信息，
/// 再存一份原文没有额外价值）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_ms: i64,
    /// 整行持续发音时长（毫秒），可能为 0。
    pub duration_ms: i64,
    pub text: String,
    /// 译文（来自 `tlyric`）。
    pub translation: Option<String>,
    /// 逐字列表；普通 LRC 为空。
    pub words: Vec<WordInfo>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &'static str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("歌词正则编译失败"))
}

fn lrc_time_tag() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\[(\d{2,}):(\d{2})[.:](\d{2,3})\]")
}

fn word_time_tag() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"<(\d{2,}):(\d{2})[.:](\d{2,3})>")
}

fn yrc_line() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^\[(\d+),(\d+)\](.*)$")
}

fn yrc_word() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\((\d+),(\d+),\d+\)([^(\n]+)")
}

fn qrc_word() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(.*?)\((\d+),(\d+)\)")
}

/// `<mm:ss.xx>` / `[mm:ss.xx]` 的毫秒换算（两位毫秒视为厘秒 ×10）。
fn time_tag_ms(caps: &regex::Captures) -> Option<i64> {
    let min: i64 = caps.get(1)?.as_str().parse().ok()?;
    let sec: i64 = caps.get(2)?.as_str().parse().ok()?;
    let ms_raw = caps.get(3)?.as_str();
    let ms: i64 = ms_raw.parse().ok()?;
    let ms = if ms_raw.len() == 2 { ms * 10 } else { ms };
    Some(min * 60_000 + sec * 1000 + ms)
}

/// 拆出行首连续的 `[mm:ss.xx]` 标签与其后的正文。
fn split_line_times(line: &str) -> (Vec<i64>, String) {
    let mut rest = line.trim();
    let mut times = Vec::new();
    while let Some(caps) = lrc_time_tag().captures(rest) {
        let m = caps.get(0).unwrap();
        if m.start() != 0 {
            break;
        }
        match time_tag_ms(&caps) {
            Some(t) => times.push(t),
            None => break,
        }
        rest = &rest[m.end()..];
    }
    (times, rest.trim().to_string())
}

/// 解析普通/增强 LRC（一行多时间标签会各自展开为一行）。
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let (times, body) = split_line_times(raw);
        if times.is_empty() || body.is_empty() {
            continue;
        }
        let words = parse_words(&body, times[0]);
        for t in times {
            lines.push(LyricLine {
                time_ms: t,
                duration_ms: words.last().map(|w| w.start_offset_ms + w.duration_ms).unwrap_or(0),
                text: body.clone(),
                translation: None,
                words: words.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// 增强 LRC 的 `<mm:ss.xx>词` 逐字；无标签返回空。
fn parse_words(body: &str, line_start_ms: i64) -> Vec<WordInfo> {
    let tags: Vec<_> = word_time_tag().captures_iter(body).collect();
    if tags.is_empty() {
        return Vec::new();
    }
    let mut words = Vec::new();
    let first = tags[0].get(0).unwrap();
    let leading = &body[..first.start()];
    if !leading.trim().is_empty() {
        words.push(WordInfo {
            text: leading.to_string(),
            start_offset_ms: 0,
            duration_ms: 0,
        });
    }
    for (i, caps) in tags.iter().enumerate() {
        let Some(start) = time_tag_ms(caps) else { continue };
        let seg_start = caps.get(0).unwrap().end();
        let seg_end = tags.get(i + 1).map(|c| c.get(0).unwrap().start()).unwrap_or(body.len());
        let segment = &body[seg_start..seg_end];
        if segment.is_empty() {
            continue;
        }
        let next = tags.get(i + 1).and_then(time_tag_ms);
        let duration = next.map(|n| (n - start).max(0)).unwrap_or(0);
        words.push(WordInfo {
            text: segment.to_string(),
            start_offset_ms: (start - line_start_ms).max(0),
            duration_ms: duration,
        });
    }
    words
}

/// 解析网易云逐字（YRC）：`[start,dur](start,dur,0)word`。
pub fn parse_yrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        let Some(caps) = yrc_line().captures(trimmed) else {
            continue;
        };
        let line_start: i64 = caps[1].parse().unwrap_or(0);
        let line_dur: i64 = caps[2].parse().unwrap_or(0);
        let content = &caps[3];

        let mut words = Vec::new();
        let mut full = String::new();
        for wc in yrc_word().captures_iter(content) {
            let abs: i64 = wc[1].parse().unwrap_or(0);
            let dur: i64 = wc[2].parse().unwrap_or(0);
            let word = wc[3].to_string();
            words.push(WordInfo {
                text: word.clone(),
                start_offset_ms: (abs - line_start).max(0),
                duration_ms: dur,
            });
            full.push_str(&word);
        }
        if full.trim().is_empty() {
            continue;
        }
        lines.push(LyricLine {
            time_ms: line_start,
            duration_ms: line_dur,
            text: full,
            translation: None,
            words,
        });
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// 解析 QQ 逐字（QRC）：`[start,dur]word(start,dur)`。
pub fn parse_qrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        let Some(caps) = yrc_line().captures(trimmed) else {
            continue;
        };
        let line_start: i64 = caps[1].parse().unwrap_or(0);
        let declared: i64 = caps[2].parse().unwrap_or(0);
        let body = &caps[3];

        let mut words = Vec::new();
        let mut full = String::new();
        let mut any = false;
        for wc in qrc_word().captures_iter(body) {
            any = true;
            let word = wc[1].to_string();
            let abs: i64 = wc[2].parse().unwrap_or(0);
            let dur: i64 = wc[3].parse().unwrap_or(0);
            full.push_str(&word);
            words.push(WordInfo {
                text: word,
                start_offset_ms: (abs - line_start).max(0),
                duration_ms: dur,
            });
        }
        let text = if any { full } else { body.to_string() };
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let duration = if declared > 0 {
            declared
        } else {
            words.last().map(|w| w.start_offset_ms + w.duration_ms).unwrap_or(0)
        };
        lines.push(LyricLine {
            time_ms: line_start,
            duration_ms: duration,
            text,
            translation: None,
            words,
        });
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// 自动嗅探格式并解析（YRC → QRC → LRC）。
pub fn parse(text: &str) -> Vec<LyricLine> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    static YRC_CHECK: OnceLock<Regex> = OnceLock::new();
    let yrc_check = re(&YRC_CHECK, r"\[\d+,\d+\]\(\d+,\d+,\d+\)");
    if yrc_check.is_match(trimmed) {
        let lines = parse_yrc(text);
        if !lines.is_empty() {
            return lines;
        }
    }
    static QRC_CHECK: OnceLock<Regex> = OnceLock::new();
    let qrc_check = re(&QRC_CHECK, r"\[\d+,\d+\][^(]*\(\d+,\d+\)");
    if qrc_check.is_match(trimmed) {
        let lines = parse_qrc(text);
        if !lines.is_empty() {
            return lines;
        }
    }
    parse_lrc(text)
}

/// 将译文按时间戳合并进主歌词。
pub fn merge_translation(lines: &mut [LyricLine], translation: &[LyricLine]) {
    use std::collections::HashMap;
    let map: HashMap<i64, &str> = translation
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| (l.time_ms, l.text.as_str()))
        .collect();
    for line in lines.iter_mut() {
        if line.translation.is_none() {
            if let Some(t) = map.get(&line.time_ms) {
                line.translation = Some((*t).to_string());
            }
        }
    }
}

/// 当前播放位置对应的歌词行下标（最后一条 `time_ms <= position`）。
pub fn current_index(lines: &[LyricLine], position_ms: i64) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    match lines.binary_search_by(|l| l.time_ms.cmp(&position_ms)) {
        Ok(i) => Some(i),
        Err(0) => None,
        Err(i) => Some(i - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_multiple_time_tags_expand() {
        let lines = parse_lrc("[00:12.00][01:30.00]副歌\n[00:20.00]第二句");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].time_ms, 12_000);
        assert_eq!(lines[0].text, "副歌");
        assert_eq!(lines[1].time_ms, 20_000);
        assert_eq!(lines[2].time_ms, 90_000);
    }

    #[test]
    fn lrc_centiseconds() {
        // 两位毫秒按厘秒 ×10
        let lines = parse_lrc("[01:02.05]x");
        assert_eq!(lines[0].time_ms, 62_050);
    }

    #[test]
    fn yrc_words_offsets() {
        let lines = parse_yrc("[1000,2000](1000,500,0)你(1500,1500,0)好");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "你好");
        assert_eq!(lines[0].duration_ms, 2000);
        assert_eq!(lines[0].words[0].start_offset_ms, 0);
        assert_eq!(lines[0].words[0].duration_ms, 500);
        assert_eq!(lines[0].words[1].start_offset_ms, 500);
    }

    #[test]
    fn qrc_words() {
        let lines = parse_qrc("[1000,2000]你(1000,500)好(1500,1500)");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "你好");
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[0].words[1].duration_ms, 1500);
    }

    #[test]
    fn auto_detect_prefers_yrc() {
        let lines = parse("[0,100](0,50,0)嗨");
        assert_eq!(lines[0].text, "嗨");
        assert_eq!(lines[0].words.len(), 1);
    }

    #[test]
    fn merge_translation_by_time() {
        let mut lines = parse_lrc("[00:10.00]hello");
        let t = parse_lrc("[00:10.00]你好");
        merge_translation(&mut lines, &t);
        assert_eq!(lines[0].translation.as_deref(), Some("你好"));
    }

    #[test]
    fn current_line_lookup() {
        let lines = parse_lrc("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");
        assert_eq!(current_index(&lines, 25_000), Some(1));
        assert_eq!(current_index(&lines, 5_000), None);
        assert_eq!(current_index(&lines, 99_000), Some(2));
    }
}
