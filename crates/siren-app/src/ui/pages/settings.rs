//! 设置页（逐步补齐）。
//!
//! 目前落地「音质」档位选择；其余分节按 `docs/08-功能对照与清单.md` 的里程碑补齐。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::AudioQuality;

use super::super::{icon, page_shell, Root};
use crate::theme;

/// 正在播放卡片编辑器行高。
const CARD_ROW_H: f32 = 44.0;

/// 拖动时的占位预览（行本身随重排实时移动，故预览极小）。
struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.0))
    }
}

impl Root {
    pub(crate) fn settings_page(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let current = self.playback.read(cx).quality(cx);
        let media_on = self.playback.read(cx).media_keys_enabled(cx);

        let weak_media = weak.clone();
        let media_control = div()
            .id("media-keys")
            .w(px(56.0))
            .flex()
            .items_center()
            .justify_center()
            .px(px(12.0))
            .py(px(6.0))
            .rounded_full()
            .cursor_pointer()
            .bg(if media_on { theme::accent() } else { theme::pane() })
            .on_click(move |_ev, _window, cx| {
                weak_media
                    .update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.set_media_keys(!media_on, cx));
                    })
                    .ok();
            })
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(if media_on { "开" } else { "关" }),
            )
            .into_any_element();

        let body = div()
            .v_flex()
            .flex_1()
            .min_w(px(0.0))
            .w_full()
            .gap(px(16.0))
            .p(px(16.0))
            .child(section(
                "音质",
                "在线播放的音质档位（更高档位需登录 / 会员）",
                quality_row(current, weak.clone()),
            ))
            .child(section("系统", "系统媒体控制（媒体键与系统播放卡片）", media_control))
            .child(section("右侧栏", "正在播放栏宽度（300–800）", self.width_control(cx)))
            .child(section(
                "正在播放卡片",
                "右侧面板卡片的顺序与显隐",
                self.cards_control(weak, cx),
            ))
            .into_any_element();

        page_shell("设置".to_string(), body).into_any_element()
    }

    /// 正在播放卡片：拖动把手调整顺序，开关控制显隐（对齐 Melodia `CardLayoutEditor`）。
    fn cards_control(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let layout = self.editor_layout(cx);

        let mut rows = div().v_flex().w_full();
        for (i, (card, visible)) in layout.iter().enumerate() {
            let card = *card;
            let visible = *visible;
            let weak_handle = weak.clone();
            let weak_switch = weak.clone();
            rows =
                rows.child(
                    div()
                        .w_full()
                        .h(px(CARD_ROW_H))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .id(("card-drag", i))
                                .size(px(28.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .on_drag(i, move |_idx, _offset, _window, cx| {
                                    weak_handle
                                        .update(cx, |root, _cx| root.set_card_drag_from(Some(i)))
                                        .ok();
                                    cx.new(|_| DragGhost)
                                })
                                .child(icon(IconName::GripVertical, 18.0, theme::text_gray())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child(card.label()),
                        )
                        .child(Switch::new(("card-switch", i)).checked(visible).on_click(
                            move |checked, _window, cx| {
                                weak_switch
                                    .update(cx, |root, cx| root.set_card_visible(card, *checked, cx))
                                    .ok();
                            },
                        )),
                );
        }

        div()
            .v_flex()
            .w_full()
            .on_drag_move::<usize>({
                let weak = weak.clone();
                move |event: &DragMoveEvent<usize>, _window, cx| {
                    let y = f32::from(event.event.position.y - event.bounds.origin.y);
                    let target = (y / CARD_ROW_H).floor().max(0.0) as usize;
                    weak.update(cx, |root, cx| {
                        let Some(from) = root.card_drag_from() else {
                            return;
                        };
                        let n = root.editor_layout(cx).len();
                        if n == 0 {
                            return;
                        }
                        let to = target.min(n - 1);
                        if to != from {
                            root.reorder_card(from, to, cx);
                            root.set_card_drag_from(Some(to));
                        }
                    })
                    .ok();
                }
            })
            .child(rows)
            .into_any_element()
    }

    /// 右侧栏宽度滑块。
    fn width_control(&self, cx: &App) -> AnyElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(div().w(px(240.0)).child(Slider::new(self.np_width_slider())))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(format!("{} px", self.np_width(cx) as i32)),
            )
            .into_any_element()
    }
}

/// 一个设置分节：标题 + 说明 + 控件。
fn section(title: &str, subtitle: &str, control: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(8.0))
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
        .child(
            div()
                .text_xs()
                .text_color(theme::text_gray())
                .child(subtitle.to_string()),
        )
        .child(control)
        .into_any_element()
}

/// 音质档位选择（pills）。
fn quality_row(current: AudioQuality, weak: WeakEntity<Root>) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap(px(8.0));
    for quality in AudioQuality::ALL {
        let active = quality == current;
        let weak = weak.clone();
        row = row.child(
            div()
                .id(("quality", quality as usize))
                .px(px(12.0))
                .py(px(6.0))
                .rounded_full()
                .cursor_pointer()
                .bg(if active { theme::accent() } else { theme::pane() })
                .hover(|s| s.bg(theme::surface_light()))
                .on_click(move |_ev, _window, cx| {
                    weak.update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.set_quality(quality, cx));
                    })
                    .ok();
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child(quality.display_name()),
                        )
                        .when(active, |s| s.child(icon(IconName::Check, 14.0, theme::text_primary()))),
                ),
        );
    }
    row.into_any_element()
}
