//! 正在播放栏（右侧）：可收起/展开的封面、曲目信息、标签页、歌词区占位。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use super::super::{cover_placeholder, fmt_date, icon, Root};
use crate::theme;

impl Root {
    pub(crate) fn now_playing(&self, weak: WeakEntity<Self>, cx: &App) -> impl IntoElement {
        if self.now_playing_open {
            let state = self.playback.read(cx).state();
            let (title, artist) = match &state.song {
                Some(song) => (song.name.clone(), song.artists.clone()),
                None => ("未在播放".to_string(), String::new()),
            };
            let cover: AnyElement = match &state.song {
                Some(song) => self.services.covers.image(&song.cover_url, 220.0),
                None => cover_placeholder(220.0).into_any_element(),
            };
            // 歌曲详情：发行时间（来自 song_detail），回退到专辑名。
            let detail = self.playback.read(cx).current_detail().cloned();
            let meta_line = match &detail {
                Some(d) if d.publish_time > 0 => {
                    format!("{} · {}", d.album, fmt_date(d.publish_time))
                }
                _ => state.song.as_ref().map(|s| s.album.clone()).unwrap_or_default(),
            };
            let weak_toggle = weak;
            // 歌词：取当前时间点附近的若干行，高亮当前行。
            let lyrics = self.playback.read(cx).current_lyrics().to_vec();
            let cur = siren_core::lyrics::current_index(&lyrics, state.position_ms);
            let lyrics_view: AnyElement = if lyrics.is_empty() {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().text_sm().text_color(theme::text_gray()).child("暂无歌词"))
                    .into_any_element()
            } else {
                let idx = cur.unwrap_or(0);
                let start = idx.saturating_sub(3);
                let end = (idx + 4).min(lyrics.len());
                let mut col = div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .overflow_hidden();
                for (i, line) in lyrics.iter().enumerate().skip(start).take(end - start) {
                    let active = i == idx;
                    let color = if active {
                        theme::text_primary()
                    } else {
                        theme::text_gray()
                    };
                    let mut item = div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.0))
                        .child(div().max_w_full().truncate().text_color(color).child(line.text.clone()));
                    if let Some(t) = &line.translation {
                        item = item.child(
                            div()
                                .max_w_full()
                                .truncate()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(t.clone()),
                        );
                    }
                    col = col.child(item);
                }
                col.into_any_element()
            };
            div()
                .v_flex()
                .w(px(340.0))
                .min_w(px(340.0))
                .h_full()
                .flex_none()
                .flex_shrink_0()
                .rounded(px(8.0))
                .bg(theme::pane())
                .p(px(16.0))
                .gap(px(14.0))
                .child(div().w_full().flex().items_center().justify_end().child(toggle_button(
                    "np-collapse",
                    IconName::ChevronRight,
                    weak_toggle,
                )))
                .child(div().w_full().flex().justify_center().child(cover))
                .child(
                    div()
                        .v_flex()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_xl()
                                .text_color(theme::text_primary())
                                .truncate()
                                .child(title),
                        )
                        .child(div().text_sm().text_color(theme::text_gray()).truncate().child(artist))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .truncate()
                                .child(meta_line),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(pill("歌曲", true))
                        .child(pill("歌词", false))
                        .child(pill("评论", false)),
                )
                .child(lyrics_view)
        } else {
            let weak_expand = weak;
            div()
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
        }
    }
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

fn pill(label: &str, active: bool) -> impl IntoElement {
    div()
        .px(px(14.0))
        .h(px(30.0))
        .flex()
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .bg(if active {
            theme::text_primary()
        } else {
            theme::surface()
        })
        .text_color(if active { theme::pane() } else { theme::text_primary() })
        .child(div().text_sm().child(label.to_string()))
}
