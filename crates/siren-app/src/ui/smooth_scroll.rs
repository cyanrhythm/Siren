//! 通用平滑滚动容器：把任意内容丢进来即可获得平滑滚动。
//!
//! Windows 滚轮是离散的 `Lines(3)` 步进、gpui 又直接瞬跳，观感很「顿」。本容器
//! 自己维护「目标偏移」，渲染期用 `transition` 缓动逼近，内容用 `.top(-offset)` 位移，
//! 不与 gpui 内建滚动耦合（避免「拦了原生又不走自己」）。
//!
//! 用法（在 `render` 等有 `window` 的地方）：
//! ```ignore
//! smooth_scroll("my-scroll", content, window, cx)
//! ```
//! - `id` 需在窗口内唯一且稳定；
//! - 容器以 `flex_1` 占满父级剩余高度（父级应为 flex 布局）。

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::base::{transition, ElementExt, Transition};
use gpui_kit::*;

use super::motion;

/// 单个滚动区的偏移状态。
#[derive(Default)]
struct Entry {
    /// 目标偏移（正数：内容上移多少像素）。
    target: f32,
    /// 内容高（prepaint 量得）。
    content: f32,
    /// 视口高（prepaint 量得）。
    viewport: f32,
}

/// 所有滚动区的状态（以 `ElementId` 为键）。
#[derive(Default)]
struct Registry(HashMap<ElementId, Entry>);

impl Global for Registry {}

/// 把 `content` 包成平滑滚动容器。
pub(crate) fn smooth_scroll(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    if cx.try_global::<Registry>().is_none() {
        cx.set_global(Registry::default());
    }
    let (target, content_h, viewport_h) = cx
        .global::<Registry>()
        .0
        .get(&id)
        .map(|entry| (entry.target, entry.content, entry.viewport))
        .unwrap_or((0.0, 0.0, 0.0));
    let max = (content_h - viewport_h).max(0.0);
    let offset = transition(
        (id.clone(), "scroll"),
        target.clamp(0.0, max),
        Transition::new(Duration::from_millis(140)).ease(motion::ease_out()),
        window,
        cx,
    );

    let wheel_id = id.clone();
    let viewport_id = id.clone();
    let content_id = id.clone();

    div()
        .id(id)
        .relative()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_hidden()
        .on_scroll_wheel(move |event, window, cx| {
            // 拦截原生滚轮，仅累积到目标偏移（由 transition 缓动）。
            cx.stop_propagation();
            let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
            let entry = cx.global_mut::<Registry>().0.entry(wheel_id.clone()).or_default();
            let max = (entry.content - entry.viewport).max(0.0);
            // 与 gpui 原生一致：滚轮向上为正增量、应减小偏移（offset 向下为负）。
            let next = (entry.target - dy).clamp(0.0, max);
            if next != entry.target {
                entry.target = next;
                window.refresh();
            }
        })
        .on_prepaint(move |bounds, _window, cx| {
            let h = f32::from(bounds.size.height);
            cx.global_mut::<Registry>()
                .0
                .entry(viewport_id.clone())
                .or_default()
                .viewport = h;
        })
        .child(
            div()
                .absolute()
                .top(px(-offset))
                .left(px(0.0))
                .right(px(0.0))
                .on_prepaint(move |bounds, _window, cx| {
                    let h = f32::from(bounds.size.height);
                    cx.global_mut::<Registry>()
                        .0
                        .entry(content_id.clone())
                        .or_default()
                        .content = h;
                })
                .child(content),
        )
}
