//! 播放队列浮层（右侧，对齐 Melodia `PlayQueuePanel.kt`）。
//!
//! 按当前位置分「已播放 / 正在播放 / 接下来播放」三段：已播放压暗、当前曲高亮；
//! 仅「接下来播放」可拖动排序；悬停行才显示拖动把手与移除按钮（对齐 Melodia `HoverReveal`）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use siren_core::model::Song;

use super::super::settings_store::SettingsStore;
use super::super::{icon, Root};
use crate::theme;

/// 队列行高（拖动排序按此换算目标下标）。
const ROW_H: f32 = 52.0;
/// 分段标题行高。
const SECTION_H: f32 = 28.0;
/// 列表内元素间距。
const GAP: f32 = 2.0;
/// 列表内边距。
const PAD: f32 = 8.0;

impl Root {
    /// 播放队列浮层。
    pub(crate) fn queue_overlay(&self, weak: WeakEntity<Self>, cx: &mut Context<Self>) -> AnyElement {
        let (items, current) = self.playback.read(cx).queue_snapshot();
        // 封面按 40px（bucket 64）渲染，需先预取（命中缓存前每次渲染幂等）。
        self.services
            .covers
            .prefetch(items.iter().map(|s| s.cover_url.clone()), 40.0, cx);
        let n = items.len();
        let current = current.filter(|c| *c < n);
        let played_end = current.unwrap_or(0);
        let upcoming_start = current.map(|c| c + 1).unwrap_or(0);

        let mut list = div().v_flex().w_full().gap(px(GAP));
        if current.is_some() && played_end > 0 {
            list = list.child(section_label("已播放"));
            for (i, song) in items[..played_end].iter().enumerate() {
                list = list.child(self.queue_row(song, i, false, true, false, &weak));
            }
        }
        if let Some(ci) = current {
            list = list.child(section_label("正在播放"));
            list = list.child(self.queue_row(&items[ci], ci, true, false, false, &weak));
        }
        if upcoming_start < n {
            list = list.child(section_label("接下来播放"));
            for (offset, song) in items[upcoming_start..].iter().enumerate() {
                let i = upcoming_start + offset;
                list = list.child(self.queue_row(song, i, false, false, true, &weak));
            }
        }

        let weak_clear = weak.clone();
        let weak_close = weak.clone();
        let header = div()
            .w_full()
            .h(px(48.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text_primary())
                    .child("播放队列"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(format!("共 {n} 首")),
            )
            .child(div().flex_1())
            .when(n > 1, |el| {
                el.child(
                    div()
                        .id("queue-clear")
                        .size(px(28.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::surface()))
                        .on_click(move |_ev, _window, cx| {
                            weak_clear
                                .update(cx, |root, cx| root.playback.update(cx, |pb, cx| pb.clear_queue(cx)))
                                .ok();
                        })
                        .child(icon(IconName::Trash, 16.0, theme::text_gray())),
                )
            })
            .child(
                div()
                    .id("queue-close")
                    .size(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_close
                            .update(cx, |root, cx| {
                                root.queue_open = false;
                                cx.notify();
                            })
                            .ok();
                    })
                    .child(icon(IconName::X, 16.0, theme::text_gray())),
            );

        let body = div()
            .id("queue-scroll")
            .v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .p(px(PAD))
            .overflow_y_scroll()
            .on_drag_move::<usize>({
                let weak = weak.clone();
                move |event: &DragMoveEvent<usize>, _window, cx| {
                    let y = f32::from(event.event.position.y - event.bounds.origin.y);
                    weak.update(cx, |root, cx| {
                        let Some(from) = root.queue_drag_from() else {
                            return;
                        };
                        let (items, current) = root.playback.read(cx).queue_snapshot();
                        let n = items.len();
                        let current = current.filter(|c| *c < n);
                        let upcoming_start = current.map(|c| c + 1).unwrap_or(0);
                        if upcoming_start >= n {
                            return;
                        }
                        // 「接下来播放」首行的内容顶偏移：内边距 + 前序分段（标题 + 行）高度 + 间距。
                        let mut top = PAD;
                        let mut kids = 0usize;
                        if let Some(c) = current.filter(|c| *c > 0) {
                            top += SECTION_H + c as f32 * ROW_H;
                            kids += 1 + c;
                        }
                        if current.is_some() {
                            top += SECTION_H + ROW_H;
                            kids += 2;
                        }
                        top += SECTION_H;
                        kids += 1;
                        top += GAP * kids as f32;
                        let stride = ROW_H + GAP;
                        let target = (((y - top) / stride).floor().max(0.0) as usize) + upcoming_start;
                        let to = target.clamp(upcoming_start, n - 1);
                        if from >= upcoming_start && to != from {
                            root.playback.update(cx, |pb, cx| pb.move_in_queue(from, to, cx));
                            root.set_queue_drag_from(Some(to));
                        }
                    })
                    .ok();
                }
            })
            .child(list);

        div()
            .absolute()
            .top(px(64.0))
            .bottom(px(theme::PLAYER_BAR_H))
            .right(px(8.0))
            .w(px(SettingsStore::of(cx).np_width()))
            .v_flex()
            .rounded(px(8.0))
            .bg(theme::pane())
            .overflow_hidden()
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// 单条队列行。`upcoming` 决定是否可拖动；`is_played` 压暗；`is_current` 高亮标题。
    fn queue_row(
        &self,
        song: &Song,
        i: usize,
        is_current: bool,
        is_played: bool,
        upcoming: bool,
        weak: &WeakEntity<Self>,
    ) -> AnyElement {
        let group = format!("queue-row-{i}");
        let weak_row = weak.clone();
        let weak_x = weak.clone();
        let weak_handle = weak.clone();
        let cover = self.services.covers.image(&song.cover_url, 40.0);

        div()
            .id(group.clone())
            .group(group.clone())
            .w_full()
            .h(px(ROW_H))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(6.0))
            .hover(|s| s.bg(theme::surface()))
            .cursor_pointer()
            .on_click(move |_ev, _window, cx| {
                weak_row
                    .update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.play_queue_at(i, cx))
                    })
                    .ok();
            })
            .child(
                div()
                    .id(("queue-drag", i))
                    .size(px(18.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    // 悬停才显形（对齐 Melodia HoverReveal）；仅「接下来播放」可拖动，其余保留占位。
                    .opacity(0.0)
                    .group_hover(group.clone(), |s| s.opacity(1.0))
                    .when(upcoming, |el| {
                        el.cursor_pointer()
                            .on_drag(i, move |_v, _offset, _window, cx| {
                                weak_handle
                                    .update(cx, |root, _cx| root.set_queue_drag_from(Some(i)))
                                    .ok();
                                cx.new(|_| QueueDragGhost)
                            })
                            .child(icon(IconName::GripVertical, 16.0, theme::text_gray()))
                    }),
            )
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
                            .text_color(if is_current {
                                theme::accent()
                            } else {
                                theme::text_primary()
                            })
                            .truncate()
                            .child(song.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .truncate()
                            .child(song.artists.clone()),
                    ),
            )
            .child(
                div()
                    .id(("queue-remove", i))
                    .size(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.0)
                    .group_hover(group, |s| s.opacity(1.0))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        cx.stop_propagation();
                        weak_x
                            .update(cx, |root, cx| {
                                root.playback.update(cx, |pb, cx| pb.remove_queue_at(i, cx))
                            })
                            .ok();
                    })
                    .child(icon(IconName::X, 14.0, theme::text_gray())),
            )
            .when(is_played, |el| el.opacity(0.5))
            .into_any_element()
    }
}

/// 分段标题（已播放 / 正在播放 / 接下来播放）。
fn section_label(text: &str) -> impl IntoElement {
    div()
        .h(px(SECTION_H))
        .w_full()
        .flex()
        .items_center()
        .px(px(8.0))
        .text_xs()
        .text_color(theme::text_gray())
        .child(text.to_string())
}

/// 队列拖动占位预览（行随重排实时移动，故预览极小）。
struct QueueDragGhost;

impl Render for QueueDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.0))
    }
}
