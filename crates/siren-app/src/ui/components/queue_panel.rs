//! 播放队列浮层（右侧，对齐 Melodia `PlayQueuePanel.kt`）。
//!
//! 按当前位置分「已播放 / 正在播放 / 接下来播放」三段：已播放压暗、当前曲高亮；
//! 仅「接下来播放」可拖动排序；悬停行才显示拖动把手与移除按钮（对齐 Melodia `HoverReveal`）。
//!
//! 拖动动效（对齐 `docs/09`）：拾起时浮动预览淡入跟随光标；重排瞬间其余行从旧槽位
//! 平滑滑到新槽位（FLIP）。二者经 `with_animation` 驱动，自动尊重系统「减少动态效果」。

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use siren_core::model::Song;

use super::super::settings_store::SettingsStore;
use super::super::smooth_scroll::{smooth_scroll, smooth_scroll_offset, smooth_scroll_to};
use super::super::{cover_placeholder, icon, motion, Root};
use super::queue_layout::{reorder_target, row_content_top, GAP, PAD, ROW_H, SECTION_H, STRIDE};
use crate::theme;

impl Root {
    /// 播放队列浮层。
    pub(crate) fn queue_overlay(
        &self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 封面预取改在「打开 / 队列结构变化」时（见 Root::set_queue_open 与 on_playback），
        // 渲染路径不再发起网络（I6）。
        let (items, current) = self.playback.read(cx).queue_snapshot();
        let n = items.len();
        let current = current.filter(|c| *c < n);
        let played_end = current.unwrap_or(0);
        let upcoming_start = current.map(|c| c + 1).unwrap_or(0);
        let keys = queue_keys(&items);

        let mut list = div().v_flex().w_full().gap(px(GAP));
        if current.is_some() && played_end > 0 {
            list = list.child(section_label("已播放"));
            for (i, song) in items[..played_end].iter().enumerate() {
                let flip = self.flip_of(&keys[i]);
                list = list.child(self.queue_row(song, &keys[i], i, false, true, false, flip, &weak));
            }
        }
        if let Some(ci) = current {
            list = list.child(section_label("正在播放"));
            let flip = self.flip_of(&keys[ci]);
            list = list.child(self.queue_row(&items[ci], &keys[ci], ci, true, false, false, flip, &weak));
        }
        if upcoming_start < n {
            list = list.child(section_label("接下来播放"));
            for (offset, song) in items[upcoming_start..].iter().enumerate() {
                let i = upcoming_start + offset;
                let flip = self.flip_of(&keys[i]);
                list = list.child(self.queue_row(song, &keys[i], i, false, false, true, flip, &weak));
            }
        }

        let weak_clear = weak.clone();
        let weak_close = weak.clone();
        let header = div()
            .w_full()
            .h(px(48.0))
            .flex()
            .items_center()
            .gap(px(theme::spacing::SM))
            .px(px(theme::spacing::MD))
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
                        .rounded(px(theme::RADIUS_SM))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::surface()))
                        .on_click(move |_ev, _window, cx| {
                            weak_clear.update(cx, |root, cx| root.open_queue_clear(cx)).ok();
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
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_close.update(cx, |root, cx| root.set_queue_open(false, cx)).ok();
                    })
                    .child(icon(IconName::X, 16.0, theme::text_gray())),
            );

        // 内容（带内边距的列表）交给平滑滚动容器；拖动排序挂在包裹层上（用当前滚动偏移做坐标补偿）。
        let content = div().v_flex().w_full().p(px(PAD)).child(list);
        let weak_drag = weak.clone();
        let body = div()
            .id("queue-body")
            .v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .on_drag_move::<usize>(move |event: &DragMoveEvent<usize>, _window, cx| {
                let y = f32::from(event.event.position.y - event.bounds.origin.y);
                let scroll = smooth_scroll_offset("queue-scroll", cx);
                weak_drag
                    .update(cx, |root, cx| {
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
                        // taffy 的 column gap 只加在相邻 kid 之间：目标行是第 kids 个 kid（从 0 计），
                        // 其上方恰好有 kids 个间隔。
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
                        let to = reorder_target(y, -scroll, top, upcoming_start, n);
                        if from >= upcoming_start && to != from {
                            let old_keys = queue_keys(&items);
                            root.playback.update(cx, |pb, cx| pb.move_in_queue(from, to, cx));
                            let (moved, _) = root.playback.read(cx).queue_snapshot();
                            let new_keys = queue_keys(&moved);
                            let drag_key = new_keys.get(to).cloned();
                            root.flip_on_reorder(&old_keys, &new_keys, STRIDE, drag_key.as_deref());
                            root.set_queue_drag_from(Some(to));
                        }
                    })
                    .ok();
            })
            .child(if n == 0 {
                // 空态：队列为空时给出去向提示（K8 同一约定）。
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .gap(px(theme::spacing::SM))
                            .child(div().text_sm().text_color(theme::text_gray()).child("队列为空"))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child("从歌单或搜索中添加歌曲开始播放"),
                            ),
                    )
                    .into_any_element()
            } else {
                smooth_scroll("queue-scroll", content, window, cx).into_any_element()
            });

        // 「清空队列」确认浮层：压暗面板局部，取消 / 清空（danger）。
        let confirm =
            self.queue_clear_open.then(|| {
                let weak_cancel = weak.clone();
                let weak_ok = weak.clone();
                div()
                    .id("queue-clear-confirm")
                    .occlude()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme::scrim())
                    .child(
                        div()
                            .v_flex()
                            .w(px(300.0))
                            .gap(px(theme::spacing::MD))
                            .rounded(px(theme::RADIUS_LG))
                            .bg(theme::surface())
                            .p(px(theme::spacing::LG))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child("清空播放队列？"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_gray())
                                    .child("队列中的所有歌曲都会被移除。"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(theme::spacing::SM))
                                    .child(Button::new("queue-clear-cancel").label("取消").on_click(
                                        move |_ev, _w, cx| {
                                            weak_cancel.update(cx, |root, cx| root.close_queue_clear(cx)).ok();
                                        },
                                    ))
                                    .child(Button::new("queue-clear-ok").danger().label("清空").on_click(
                                        move |_ev, _w, cx| {
                                            weak_ok.update(cx, |root, cx| root.confirm_queue_clear(cx)).ok();
                                        },
                                    )),
                            ),
                    )
                    .into_any_element()
            });

        // 面板宽与右栏有效宽一致（窗口收窄时随动，见 Root::np_effective_w）。
        let pads = gpui_kit::component::window_paddings(window);
        let vw = f32::from(window.viewport_size().width) - f32::from(pads.left) - f32::from(pads.right);
        let np_w = self.np_effective_w(vw, cx);
        div()
            .absolute()
            .top(px(64.0))
            .bottom(px(theme::PLAYER_BAR_H))
            .right(px(theme::spacing::SM))
            .w(px(np_w))
            .v_flex()
            .rounded(px(theme::RADIUS_MD))
            .bg(theme::pane())
            .overflow_hidden()
            .child(header)
            .child(body)
            .when_some(confirm, |el, confirm| el.child(confirm))
            .into_any_element()
    }

    /// 打开队列浮层时把滚动定位到当前曲（内容首帧量出后生效，见 smooth_scroll_to）。
    pub(crate) fn scroll_queue_to_current(&self, cx: &mut App) {
        let (items, current) = self.playback.read(cx).queue_snapshot();
        let n = items.len();
        if n == 0 {
            return;
        }
        let index = current.filter(|c| *c < n).unwrap_or(0);
        let Some(top) = row_content_top(index, current, n) else {
            return;
        };
        smooth_scroll_to("queue-scroll", (top - PAD).max(0.0), cx);
    }

    /// 单条队列行。`upcoming` 决定是否可拖动；`is_played` 压暗；`is_current` 高亮标题；
    /// `flip` 为重排归位补间（起始偏移, 代次）。
    #[allow(clippy::too_many_arguments)]
    fn queue_row(
        &self,
        song: &Song,
        key: &str,
        i: usize,
        is_current: bool,
        is_played: bool,
        upcoming: bool,
        flip: Option<(f32, u32)>,
        weak: &WeakEntity<Self>,
    ) -> AnyElement {
        let group = format!("queue-row-{i}");
        let weak_row = weak.clone();
        let weak_x = weak.clone();
        let cover = self.services.covers.image(&song.cover_url, 40.0);

        let row = div()
            .id(group.clone())
            .group(group.clone())
            .w_full()
            .h(px(ROW_H))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(theme::RADIUS_SM))
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
                        let name = song.name.clone();
                        let artists = song.artists.clone();
                        let cover = self.services.covers.image_arc(&song.cover_url, 40.0);
                        let weak_handle = weak.clone();
                        el.cursor_pointer()
                            .on_drag(i, move |_v, _offset, _window, cx| {
                                weak_handle
                                    .update(cx, |root, _cx| {
                                        root.clear_drag_flip();
                                        root.set_queue_drag_from(Some(i));
                                    })
                                    .ok();
                                cx.new(|_| QueueDragPreview {
                                    name: name.clone(),
                                    artists: artists.clone(),
                                    cover: cover.clone(),
                                })
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
            .when(is_played, |el| el.opacity(0.5));

        // 重排归位：从旧槽位的偏移滑回 0（屏幕内移动 → 强 ease-in-out）。
        match flip {
            Some((from, generation)) if from != 0.0 => row
                .with_animation(
                    ElementId::Name(format!("queue-flip-{key}-{generation}").into()),
                    Animation::new(motion::REORDER).with_easing(motion::ease_in_out()),
                    move |el, delta| el.top(px(from * (1.0 - delta))),
                )
                .into_any_element(),
            _ => row.into_any_element(),
        }
    }
}

/// 分段标题（已播放 / 正在播放 / 接下来播放）。
fn section_label(text: &str) -> impl IntoElement {
    div()
        .h(px(SECTION_H))
        .w_full()
        .flex()
        .items_center()
        .px(px(theme::spacing::SM))
        .text_xs()
        .text_color(theme::text_gray())
        .child(text.to_string())
}

/// 队列稳定键：同一首歌重复入队时以出现次数区分。
fn queue_keys(items: &[Song]) -> Vec<String> {
    let mut seen: HashMap<i64, usize> = HashMap::new();
    items
        .iter()
        .map(|song| {
            let occurrence = seen.entry(song.id).or_insert(0);
            let key = format!("q:{}:{}", song.id, *occurrence);
            *occurrence += 1;
            key
        })
        .collect()
}

/// 拖动拾起时跟随光标的浮动行预览（拾起入场：淡入 + 轻微上浮）。
struct QueueDragPreview {
    name: String,
    artists: String,
    cover: Option<Arc<Image>>,
}

impl Render for QueueDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = SettingsStore::of(cx).np_width();
        let cover = match &self.cover {
            Some(image) => img(image.clone())
                .w(px(40.0))
                .h(px(40.0))
                .rounded(px(theme::RADIUS_SM))
                .into_any_element(),
            None => cover_placeholder(40.0).into_any_element(),
        };
        div()
            .w(px(width))
            .h(px(ROW_H))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme::pane())
            .shadow_lg()
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
                            .child(self.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .truncate()
                            .child(self.artists.clone()),
                    ),
            )
            .with_animation(
                "queue-drag-preview",
                Animation::new(motion::DRAG_ENTER).with_easing(motion::ease_out()),
                |el, delta| el.opacity(delta).top(px(-8.0 * (1.0 - delta))),
            )
    }
}
