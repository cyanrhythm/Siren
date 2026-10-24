//! 离线缓存提示：详情页在「在线请求失败、改用磁盘快照」时显示的一枚 chip。
//!
//! 只做展示，不参与交互——目的是让用户知道「现在看到的是旧数据」，
//! 而不是把「接口失败」误当成「歌单里就这些歌」。

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::icon;
use crate::theme::{self, spacing};

/// `离线缓存` 提示（灰色 chip，居中不占位高度以外的空间）。
pub(crate) fn offline_hint() -> AnyElement {
    div()
        .id("offline-hint")
        .flex()
        .items_center()
        .gap(px(spacing::XXS))
        .px(px(spacing::SM))
        .h(px(24.0))
        .rounded_full()
        .bg(theme::surface())
        .child(icon(IconName::Info, 14.0, theme::text_gray()))
        .child(
            div()
                .text_xs()
                .text_color(theme::text_gray())
                .child("离线缓存 · 可能不是最新"),
        )
        .into_any_element()
}
