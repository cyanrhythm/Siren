//! 底部播放条。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::*;

use siren_core::player::PlayMode;

use super::super::{cover_placeholder, fmt_time, icon, Root};
use crate::theme::{self, spacing};

/// 透明底图标键：自定义前景色（激活态高亮）+ 统一 hover 底。
fn transport_variant(cx: &App, fg: Rgba) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(fg.into())
        .hover(theme::surface_light().into())
}

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
        // 拖动进度条时左侧时间跟随预览位置。
        let pos_label = pb
            .seek_preview_ms()
            .map(fmt_time)
            .unwrap_or_else(|| fmt_time(state.position_ms));
        let dur_label = fmt_time(state.duration_ms);
        let progress_w = (vw - 210.0 - 300.0 - 240.0 - 64.0).max(160.0);
        let cover: AnyElement = match &state.song {
            Some(song) => self.services.covers.image(&song.cover_url, 56.0),
            None => cover_placeholder(56.0).into_any_element(),
        };

        let weak_play = weak.clone();
        let weak_prev = weak.clone();
        let weak_shuffle = weak.clone();
        let weak_repeat = weak.clone();
        let weak_queue = weak.clone();
        let weak_next = weak;
        let queue_open = self.queue_open;
        let shuffle_active = state.mode == PlayMode::Shuffle;
        let repeat_active = state.mode == PlayMode::SingleLoop;
        div()
            .w_full()
            .h(px(theme::PLAYER_BAR_H))
            .flex()
            .items_center()
            .gap(px(spacing::MD))
            .px(px(spacing::MD))
            .child(
                div()
                    .w(px(300.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(cover)
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
                    .gap(px(spacing::SM))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .child(
                                Button::new("shuffle")
                                    .custom(transport_variant(
                                        cx,
                                        if shuffle_active {
                                            theme::accent()
                                        } else {
                                            theme::text_gray()
                                        },
                                    ))
                                    .with_size(Size::Size(px(24.0)))
                                    .icon(IconName::Shuffle)
                                    .tooltip("随机播放")
                                    .on_click(move |_ev, _window, cx| {
                                        weak_shuffle
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.toggle_shuffle(cx));
                                            })
                                            .ok();
                                    }),
                            )
                            .child(
                                Button::new("prev")
                                    .custom(transport_variant(cx, theme::text_primary()))
                                    .with_size(Size::Size(px(28.0)))
                                    .icon(IconName::SkipBack)
                                    .tooltip("上一首")
                                    .on_click(move |_ev, _window, cx| {
                                        weak_prev
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.previous_track(cx));
                                            })
                                            .ok();
                                    }),
                            )
                            .child(
                                Button::new("play")
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(theme::text_primary().into())
                                            .foreground(theme::pane().into())
                                            .hover(theme::surface_light().into()),
                                    )
                                    .with_size(Size::Size(px(36.0)))
                                    .rounded(px(18.0))
                                    .icon(if playing { IconName::Pause } else { IconName::Play })
                                    .tooltip(if playing { "暂停" } else { "播放" })
                                    .on_click(move |_ev, _window, cx| {
                                        weak_play
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.toggle(cx));
                                            })
                                            .ok();
                                    }),
                            )
                            .child(
                                Button::new("next")
                                    .custom(transport_variant(cx, theme::text_primary()))
                                    .with_size(Size::Size(px(28.0)))
                                    .icon(IconName::SkipForward)
                                    .tooltip("下一首")
                                    .on_click(move |_ev, _window, cx| {
                                        weak_next
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.next_track(cx));
                                            })
                                            .ok();
                                    }),
                            )
                            .child(
                                Button::new("repeat")
                                    .custom(transport_variant(
                                        cx,
                                        if repeat_active {
                                            theme::accent()
                                        } else {
                                            theme::text_gray()
                                        },
                                    ))
                                    .with_size(Size::Size(px(24.0)))
                                    .icon(IconName::Repeat)
                                    .tooltip("循环模式")
                                    .on_click(move |_ev, _window, cx| {
                                        weak_repeat
                                            .update(cx, |this, cx| {
                                                this.playback.update(cx, |pb, cx| pb.toggle_repeat(cx));
                                            })
                                            .ok();
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
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
                    .gap(px(spacing::SM))
                    .child(
                        Button::new("queue-btn")
                            .custom(transport_variant(
                                cx,
                                if queue_open {
                                    theme::accent()
                                } else {
                                    theme::text_gray()
                                },
                            ))
                            .with_size(Size::Size(px(24.0)))
                            .icon(IconName::ListMusic)
                            .tooltip("播放队列")
                            .on_click(move |_ev, _window, cx| {
                                weak_queue
                                    .update(cx, |root, cx| {
                                        let open = !root.queue_open;
                                        root.set_queue_open(open, cx);
                                    })
                                    .ok();
                            }),
                    )
                    .child(icon(IconName::Volume2, 18.0, theme::text_gray()))
                    .child(div().w(px(120.0)).child(Slider::new(pb.volume_slider()))),
            )
    }
}
