//! 设置页（逐步补齐）。
//!
//! 目前落地「音质」档位选择；其余分节按 `docs/08-功能对照与清单.md` 的里程碑补齐。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::AudioQuality;

use super::super::{icon, page_shell, Root};
use crate::theme;

impl Root {
    pub(crate) fn settings_page(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let current = self.playback.read(cx).quality();

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
                quality_row(current, weak),
            ))
            .into_any_element();

        page_shell("设置".to_string(), body).into_any_element()
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
