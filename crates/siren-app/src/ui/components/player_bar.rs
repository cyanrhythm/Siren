//! 底部播放条。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::slider::Slider;
use gpui_kit::*;

use super::super::{cover_placeholder, fmt_time, icon, Root};
use crate::theme;

impl Root {
    pub(crate) fn player_bar(&self, weak: WeakEntity<Self>, vw: f32, cx: &App) -> impl IntoElement {
        let pb = self.playback.read(cx);
        let state = pb.state();
        let playing = state.playing;
        let title = state
            .song
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未在播放".to_string());
        let artist = state.song.as_ref().map(|s| s.artists.clone()).unwrap_or_default();
        let pos_label = fmt_time(state.position_ms);
        let dur_label = fmt_time(state.duration_ms);
        let progress_w = (vw - 210.0 - 300.0 - 240.0 - 64.0).max(160.0);

        let weak_play = weak.clone();
        let weak_next = weak;
        div()
            .w_full()
            .h(px(theme::PLAYER_BAR_H))
            .flex()
            .items_center()
            .gap(px(16.0))
            .px(px(16.0))
            .child(
                div()
                    .w(px(300.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(cover_placeholder(56.0))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .truncate()
                                    .child(title),
                            )
                            .child(div().text_xs().text_color(theme::text_gray()).truncate().child(artist)),
                    ),
            )
            .child(
                div()
                    .w(px(progress_w + 120.0))
                    .flex_none()
                    .v_flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(16.0))
                            .child(icon(IconName::SkipBack, 22.0, theme::text_primary()))
                            .child(
                                div()
                                    .id("play")
                                    .size(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(theme::text_primary())
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak_play
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.toggle(cx));
                                            })
                                            .ok();
                                    })
                                    .child(icon(
                                        if playing { IconName::Pause } else { IconName::Play },
                                        18.0,
                                        theme::pane(),
                                    )),
                            )
                            .child(
                                div()
                                    .id("next")
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak_next
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.next_track(cx));
                                            })
                                            .ok();
                                    })
                                    .child(icon(IconName::SkipForward, 22.0, theme::text_primary())),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .w(px(40.0))
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(pos_label),
                            )
                            .child(div().w(px(progress_w)).child(Slider::new(pb.progress_slider())))
                            .child(
                                div()
                                    .w(px(40.0))
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(dur_label),
                            ),
                    ),
            )
            .child(
                div()
                    .w(px(240.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .child(icon(IconName::Volume2, 18.0, theme::text_gray()))
                    .child(div().w(px(120.0)).child(Slider::new(pb.volume_slider()))),
            )
    }
}
