//! 正在播放栏（右侧）：可上下滚动的卡片堆叠。
//!
//! 对齐 Melodia `desktopApp/ui/NowPlayingPanel.kt`：封面 + 标题/歌手 + 信息卡
//! （歌词预览 / 评论 / 歌曲详情 / 回忆坐标），整体竖向滚动。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::{Song, SongWiki};

use super::super::{cover_placeholder, fmt_date, icon, Root};
use crate::theme;

impl Root {
    pub(crate) fn now_playing(&self, weak: WeakEntity<Self>, cx: &App) -> impl IntoElement {
        if !self.now_playing_open {
            let weak_expand = weak;
            return div()
                .v_flex()
                .w(px(44.0))
                .min_w(px(44.0))
                .h_full()
                .flex_none()
                .flex_shrink_0()
                .rounded(px(8.0))
                .bg(theme::pane())
                .items_center()
                .justify_center()
                .child(toggle_button("np-expand", IconName::ChevronLeft, weak_expand))
                .into_any_element();
        }

        let state = self.playback.read(cx).state();
        let song = state.song.clone();
        let (title, artist) = match &song {
            Some(s) => (s.name.clone(), s.artists.clone()),
            None => ("未在播放".to_string(), String::new()),
        };
        let cover: AnyElement = match &song {
            Some(s) => self.services.covers.image(&s.cover_url, 280.0),
            None => cover_placeholder(280.0).into_any_element(),
        };
        let detail = self.playback.read(cx).current_detail().cloned();
        let lyrics = self.playback.read(cx).current_lyrics().to_vec();
        let wiki = self.playback.read(cx).current_wiki().cloned();

        // 卡片流（竖向滚动）。
        let mut body = div()
            .id("np-scroll")
            .v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .gap(px(14.0))
            .overflow_y_scroll()
            .child(div().w_full().flex().justify_center().child(cover))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_color(theme::text_primary())
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    )
                    .child(div().text_sm().text_color(theme::text_gray()).truncate().child(artist)),
            );

        if !lyrics.is_empty() {
            body = body.child(lyrics_card(&lyrics, state.position_ms));
        }
        body = body.child(comments_card());
        if let Some(card) = song_detail_card(wiki.as_ref(), song.as_ref(), detail.as_ref()) {
            body = body.child(card);
        }

        div()
            .v_flex()
            .w(px(340.0))
            .min_w(px(340.0))
            .h_full()
            .flex_none()
            .flex_shrink_0()
            .rounded(px(8.0))
            .bg(theme::pane())
            .p(px(12.0))
            .gap(px(8.0))
            .child(div().w_full().flex().items_center().justify_end().child(toggle_button(
                "np-collapse",
                IconName::ChevronRight,
                weak,
            )))
            .child(body)
            .into_any_element()
    }
}

/// 信息卡外壳：圆角底 + 标题 + 内容（对齐 `InfoCard`）。
fn info_card(title: &str, content: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(8.0))
        .bg(theme::card_surface())
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(content)
        .into_any_element()
}

/// 歌词预览卡：当前行附近若干行，当前行高亮。
fn lyrics_card(lines: &[siren_core::lyrics::LyricLine], position_ms: i64) -> AnyElement {
    let idx = siren_core::lyrics::current_index(lines, position_ms).unwrap_or(0);
    let start = idx.saturating_sub(1);
    let end = (idx + 4).min(lines.len());
    let mut col = div().v_flex().w_full().gap(px(6.0)).overflow_hidden();
    for (i, line) in lines.iter().enumerate().skip(start).take(end - start) {
        let active = i == idx;
        let color = if active {
            theme::text_primary()
        } else {
            theme::text_gray()
        };
        let mut item = div()
            .v_flex()
            .w_full()
            .gap(px(2.0))
            .child(div().w_full().truncate().text_color(color).child(line.text.clone()));
        if let Some(t) = &line.translation {
            item = item.child(
                div()
                    .w_full()
                    .truncate()
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(t.clone()),
            );
        }
        col = col.child(item);
    }
    info_card("歌词预览", col.into_any_element())
}

/// 评论预览卡（评论 API 待接入，先给入口）。
fn comments_card() -> AnyElement {
    info_card(
        "评论",
        div()
            .w_full()
            .py(px(8.0))
            .child(div().text_sm().text_color(theme::text_gray()).child("评论待接入"))
            .into_any_element(),
    )
}

/// 歌曲详情卡：曲风/专辑/语种/发行时间/BPM/影综/获奖/制作人员（缺失项不显示）。
fn song_detail_card(wiki: Option<&SongWiki>, song: Option<&Song>, detail: Option<&Song>) -> Option<AnyElement> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let album = wiki
        .map(|w| w.album.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| song.map(|s| s.album.clone()))
        .unwrap_or_default();
    if let Some(w) = wiki {
        if !w.style.is_empty() {
            rows.push(("曲风".into(), w.style.clone()));
        }
    }
    if !album.is_empty() {
        rows.push(("专辑".into(), album));
    }
    if let Some(w) = wiki {
        if !w.language.is_empty() {
            rows.push(("语种".into(), w.language.clone()));
        }
    }
    let publish = wiki
        .map(|w| w.publish_time.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| detail.filter(|d| d.publish_time > 0).map(|d| fmt_date(d.publish_time)))
        .unwrap_or_default();
    if !publish.is_empty() {
        rows.push(("发行时间".into(), publish));
    }
    if let Some(w) = wiki {
        if !w.bpm.is_empty() {
            rows.push(("BPM".into(), w.bpm.clone()));
        }
        if !w.entertainment.is_empty() {
            rows.push(("影综".into(), w.entertainment.clone()));
        }
        if !w.awards.is_empty() {
            let mut v = w.awards.join(" / ");
            if w.award_total as usize > w.awards.len() {
                v = format!("{v}（共 {} 项）", w.award_total);
            }
            rows.push(("获奖".into(), v));
        }
        if !w.creator_roles.is_empty() {
            let creators = w
                .creator_roles
                .iter()
                .map(|r| format!("{} {}", r.role, r.artists.join(" ")))
                .collect::<Vec<_>>()
                .join(" / ");
            rows.push(("制作人员".into(), creators));
        }
    }
    if rows.is_empty() {
        return None;
    }
    let mut col = div().v_flex().w_full().gap(px(6.0));
    for (label, value) in rows {
        col = col.child(
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(64.0))
                        .flex_none()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_sm()
                        .text_color(theme::text_primary())
                        .child(value),
                ),
        );
    }
    Some(info_card("歌曲详情", col.into_any_element()))
}

fn toggle_button(id: &'static str, name: IconName, weak: WeakEntity<Root>) -> impl IntoElement {
    div()
        .id(id)
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface()))
        .on_click(move |_ev, _window, cx| {
            weak.update(cx, |root, cx| root.toggle_now_playing(cx)).ok();
        })
        .child(icon(name, 16.0, theme::text_gray()))
}
