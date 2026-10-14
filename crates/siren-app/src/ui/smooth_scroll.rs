//! 通用平滑滚动。
//!
//! Windows 滚轮是离散的 `Lines(3)` 步进、gpui 又直接瞬跳，观感很「顿」。统一做法：自己维护
//! 「目标偏移」，渲染期用 `transition` 缓动逼近，可被下一格增量打断。两个入口：
//!
//! - [`smooth_scroll`]：把任意内容包成容器（内容 `.top(-offset)` 位移）；
//! - [`smooth_virtual_list`]：可平滑的虚拟化列表（只渲染可见行，替代无法驱动的 `uniform_list`）。
//!
//! 用法（在 `render` 等有 `window` 的地方）：
//! ```ignore
//! smooth_scroll("my-scroll", content, window, cx)
//! smooth_virtual_list("my-list", item_count, 52.0, move |range, w, cx| rows, window, cx)
//! ```
//! `id` 需在窗口内唯一且稳定；容器以 `flex_1` 占满父级剩余高度（父级应为 flex 布局）。

use std::collections::HashMap;
use std::ops::Range;
use std::time::Duration;

use gpui_kit::base::{transition, ElementExt, StyledExt as _, Transition};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::motion;
use crate::theme;

/// 单个滚动区的状态。
#[derive(Default)]
struct Entry {
    /// 目标偏移（正数：内容上移多少像素）。
    target: f32,
    /// 上一次渲染实际使用的偏移（缓动后的值；供拖动排序等做坐标补偿）。
    offset: f32,
    /// 内容高（`smooth_scroll` 在 prepaint 量得；虚拟列表按行高×数量直接给）。
    content: f32,
    /// 视口高（prepaint 量得）。
    viewport: f32,
    /// 最大可滚偏移（正的像素数）。
    max: f32,
}

/// 所有滚动区的状态（以 `ElementId` 为键）。
#[derive(Default)]
struct Registry(HashMap<ElementId, Entry>);

impl Global for Registry {}

fn ensure_registry(cx: &mut App) {
    if cx.try_global::<Registry>().is_none() {
        cx.set_global(Registry::default());
    }
}

/// 读取某滚动区当前偏移（缓动后的值）；未注册返回 0。
pub(crate) fn smooth_scroll_offset(id: impl Into<ElementId>, cx: &App) -> f32 {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.0.get(&id.into()).map(|entry| entry.offset))
        .unwrap_or(0.0)
}

/// 右侧细滚动条：`offset/max` 决定滑块位置，`viewport/content` 决定滑块长度。
fn scrollbar(offset: f32, max: f32, viewport: f32) -> Option<impl IntoElement> {
    if max <= 0.5 || viewport <= 0.0 {
        return None;
    }
    let content = viewport + max;
    let thumb_h = (viewport * viewport / content).clamp(24.0, viewport);
    let travel = (viewport - thumb_h).max(0.0);
    let top = (offset / max).clamp(0.0, 1.0) * travel;
    Some(
        div()
            .absolute()
            .top(px(top))
            .right(px(2.0))
            .w(px(4.0))
            .h(px(thumb_h))
            .rounded(px(2.0))
            .bg(theme::text_gray())
            .opacity(0.5),
    )
}

/// 把 `content` 包成平滑滚动容器。
pub(crate) fn smooth_scroll(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    ensure_registry(cx);
    let (target, content_h, viewport_h) = cx
        .global::<Registry>()
        .0
        .get(&id)
        .map(|entry| (entry.target, entry.content, entry.viewport))
        .unwrap_or((0.0, 0.0, 0.0));
    let max = (content_h - viewport_h).max(0.0);
    {
        let registry = cx.global_mut::<Registry>();
        let entry = registry.0.entry(id.clone()).or_default();
        entry.max = max;
        entry.target = entry.target.clamp(0.0, max);
    }
    let offset = transition(
        (id.clone(), "scroll"),
        target.clamp(0.0, max),
        Transition::new(Duration::from_millis(140)).ease(motion::ease_out()),
        window,
        cx,
    );
    cx.global_mut::<Registry>().0.entry(id.clone()).or_default().offset = offset;

    let wheel_id = id.clone();
    let viewport_id = id.clone();
    let content_id = id.clone();
    let bar = scrollbar(offset, max, viewport_h);

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
            // 与 gpui 原生一致：滚轮向上为正增量、应减小偏移（offset 向下为负）。
            let next = (entry.target - dy).clamp(0.0, entry.max);
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
        .when_some(bar, |el, bar| el.child(bar))
}

/// 可平滑滚动的**虚拟列表**：只渲染可见行，偏移由自己维护（因此能套 `transition` 缓动）。
///
/// 用于替代 `uniform_list`（后者自管偏移、无法被驱动）。`row_height` 为等高行高；
/// `render_range` 与 `uniform_list` 的闭包一致，返回该范围的行元素。
pub(crate) fn smooth_virtual_list(
    id: impl Into<ElementId>,
    item_count: usize,
    row_height: f32,
    render_range: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = id.into();
    ensure_registry(cx);
    let content_h = item_count as f32 * row_height;
    let viewport_h = cx.global::<Registry>().0.get(&id).map_or(0.0, |e| e.viewport);
    let max = (content_h - viewport_h).max(0.0);
    let target = {
        let registry = cx.global_mut::<Registry>();
        let entry = registry.0.entry(id.clone()).or_default();
        entry.content = content_h;
        entry.max = max;
        entry.target.clamp(0.0, max)
    };
    let offset = transition(
        (id.clone(), "scroll"),
        target,
        Transition::new(Duration::from_millis(140)).ease(motion::ease_out()),
        window,
        cx,
    );
    cx.global_mut::<Registry>().0.entry(id.clone()).or_default().offset = offset;

    // 仅渲染可见范围（首行 + 视口内行数，多预读 2 行）。
    let first = (offset / row_height).floor().max(0.0) as usize;
    let visible = (viewport_h / row_height).ceil() as usize + 2;
    let last = (first + visible).min(item_count);
    let rows = if item_count == 0 || first >= last {
        Vec::new()
    } else {
        render_range(first..last, window, cx)
    };
    let content = div()
        .absolute()
        .top(px(first as f32 * row_height - offset))
        .left(px(0.0))
        .right(px(0.0))
        .v_flex()
        .children(rows);

    let wheel_id = id.clone();
    let viewport_id = id.clone();
    let bar = scrollbar(offset, max, viewport_h);

    div()
        .id(id)
        .relative()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_hidden()
        .on_scroll_wheel(move |event, window, cx| {
            cx.stop_propagation();
            let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
            let entry = cx.global_mut::<Registry>().0.entry(wheel_id.clone()).or_default();
            let next = (entry.target - dy).clamp(0.0, entry.max);
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
        .child(content)
        .when_some(bar, |el, bar| el.child(bar))
}
