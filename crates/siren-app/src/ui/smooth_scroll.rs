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
//!
//! 右侧细滚动条：滚动时（短暂驻留）或指针靠近右缘时出现。按住滑块后进入「拖动捕获」——
//! 借 drag 系统的全局 `on_drag_move`，指针离开那条细线甚至离开滚动区都持续跟随，直到松开左键。

use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

use gpui_kit::base::{transition, ElementExt, StyledExt as _, Transition};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::motion;
use crate::theme;

/// 滚动条命中区域宽度（贴近右缘）。
const BAR_TRACK_W: f32 = 8.0;
/// 滚动条滑块宽度。
const BAR_THUMB_W: f32 = 4.0;
/// 滑块最短长度。
const MIN_THUMB: f32 = 32.0;
/// 滚动后滚动条驻留时长。
const BAR_HOLD: Duration = Duration::from_millis(900);
/// 滚轮缓动时长；拖动时改为 0（跟手）。
const SCROLL_EASE: Duration = Duration::from_millis(140);
const BAR_FADE: Duration = Duration::from_millis(150);

/// 拖动滚动条的 drag 载荷（类型即标识）。
struct ScrollbarDrag;
/// 拖动时的浮动预览（隐藏，仅用于挂接 drag 系统）。
struct ScrollbarPreview;

impl Render for ScrollbarPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(0.0)).h(px(0.0))
    }
}

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
    /// 滚动条命中区（窗口坐标）：左右上下。
    bar_left: f32,
    bar_top: f32,
    bar_bottom: f32,
    /// 最大可滚偏移（正的像素数）。
    max: f32,
    /// 展开态下的最大可滚偏移（= 内容高 − 展开态视口），供折叠决策使用。
    max_expanded: f32,
    /// 页面在本帧写入的头部收起量（列表视口增量）。
    header_growth: f32,
    /// 与 `viewport` 同帧记录的收起量：`viewport - growth_for_layout` 才是展开态视口。
    growth_for_layout: f32,
    /// 滑块长度 / 可移动距离。
    thumb_h: f32,
    travel: f32,
    /// 指针是否停在右缘滚动条上。
    bar_hover: bool,
    /// 是否处于「滚动后驻留」期。
    scrolling: bool,
    /// 驻留计时器的代次，用于让连续滚动延长驻留。
    hold_gen: u64,
    /// 正在拖动滚动条时的抓取点（相对滑块顶部的像素）。
    drag_grab: Option<f32>,
    /// 是否正在拖动滚动条（拖动时偏移跟手、不缓动）。
    dragging: bool,
    /// 待生效的目标偏移：内容尚未量出（max=0）时先挂起，首帧量出后恢复（N5.6）。
    pending: Option<f32>,
    /// 诊断：上次输出的（行数, 视口, 量程, 目标）快照，仅变化时打日志。
    diag_last: Option<(usize, u32, u32, u32)>,
    /// 诊断：量程抖动追踪（上次 max / 时刻 / 方向 / 近 1s 方向翻转次数）。
    diag_max: f32,
    diag_at: Option<Instant>,
    diag_dir: i8,
    diag_flips: u32,
}

impl Entry {
    fn bar_visible(&self) -> bool {
        self.max > 0.5 && (self.bar_hover || self.scrolling || self.dragging)
    }

    fn thumb_top(&self) -> f32 {
        if self.max > 0.0 {
            (self.offset / self.max).clamp(0.0, 1.0) * self.travel
        } else {
            0.0
        }
    }

    /// 指针是否落在右缘滚动条命中区内。
    fn hits_bar(&self, pos: Point<Pixels>) -> bool {
        let x = f32::from(pos.x);
        let y = f32::from(pos.y);
        x >= self.bar_left && x <= self.bar_left + BAR_TRACK_W && y >= self.bar_top && y <= self.bar_bottom
    }
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

fn entry_id(name: &'static str) -> ElementId {
    ElementId::Name(name.into())
}

/// 读取某滚动区当前偏移（缓动后的值）；未注册返回 0。
pub(crate) fn smooth_scroll_offset(id: impl Into<ElementId>, cx: &App) -> f32 {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.0.get(&id.into()).map(|entry| entry.offset))
        .unwrap_or(0.0)
}

/// 折叠头是否值得启用。
///
/// 折叠会增大列表视口、同步缩小量程；若「展开态量程」不足以在收起后仍高过
/// 折叠阈值，偏移会被钳回阈值以下 → 折叠展开 → 量程复原 → 再折叠，来回振荡，
/// 底部永远差一截。判据：`展开态量程 > 收起后视口增量 + 操作栏阈值`。
pub(crate) fn header_fold_allowed(max_expanded: f32, viewport_growth: f32, action_threshold: f32) -> bool {
    max_expanded > viewport_growth + action_threshold
}

/// 页面把本帧的头部收起量（列表视口增量）写进来；列表在 prepaint 时与视口一起落账。
pub(crate) fn set_header_growth(name: &'static str, growth: f32, cx: &mut App) {
    ensure_registry(cx);
    cx.global_mut::<Registry>()
        .0
        .entry(entry_id(name))
        .or_default()
        .header_growth = growth;
}

/// 展开态最大可滚偏移（最近一次列表渲染写入）。
pub(crate) fn smooth_scroll_max_expanded(name: &'static str, cx: &App) -> f32 {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.0.get(&entry_id(name)).map(|entry| entry.max_expanded))
        .unwrap_or(0.0)
}

/// 置「滚动后驻留」并（重新）安排驻留结束；用于滚轮、拖动等任何应显形滚动条的操作。
fn reveal_bar(name: &'static str, window: &mut Window, cx: &mut App) {
    let gen = {
        let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
        entry.scrolling = true;
        entry.hold_gen = entry.hold_gen.wrapping_add(1);
        entry.hold_gen
    };
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(BAR_HOLD).await;
            cx.update(|window, cx| {
                let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
                if entry.hold_gen == gen {
                    entry.scrolling = false;
                    window.refresh();
                }
            })
            .ok();
        })
        .detach();
}

fn end_drag(name: &'static str, window: &mut Window, cx: &mut App) {
    let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
    if entry.dragging || entry.drag_grab.is_some() {
        entry.dragging = false;
        entry.drag_grab = None;
        window.refresh();
    }
}

/// 右侧滚动条：滑块随偏移移动；按住可拖动（拖动由全局 `on_drag_move` 接管）。
fn scroll_bar(name: &'static str, thumb_h: f32, thumb_top: f32, opacity: f32) -> impl IntoElement {
    let base = entry_id(name);
    let thumb = div()
        .absolute()
        .top(px(thumb_top))
        .right(px(theme::spacing::XXS))
        .w(px(BAR_THUMB_W))
        .h(px(thumb_h))
        .rounded(px(BAR_THUMB_W / 2.0))
        .bg(theme::text_gray())
        .opacity(opacity);

    let top_base = base.clone();
    let down_base = base.clone();
    let drag_base = base.clone();
    let exit_base = base;

    div()
        .id(ElementId::Name(format!("{name}-bar").into()))
        .absolute()
        .top(px(0.0))
        .bottom(px(0.0))
        .right(px(0.0))
        .w(px(BAR_TRACK_W))
        .cursor_pointer()
        .on_prepaint(move |bounds, _window, cx| {
            let entry = cx
                .global_mut::<Registry>()
                .0
                .entry(top_base.clone())
                .or_default();
            entry.bar_left = f32::from(bounds.origin.x);
            entry.bar_top = f32::from(bounds.origin.y);
            entry.bar_bottom = f32::from(bounds.origin.y + bounds.size.height);
        })
        // 仅用于在悬停变化时请求重绘；显形与否由 render 依真实光标位置判定。
        .on_hover(move |_hovered, window, _cx| {
            window.refresh();
        })
        // 按下：若在滑块上则抓取该点，否则以滑块中心跳转（QQ/系统惯例）。
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            let reveal = {
                let entry = cx
                    .global_mut::<Registry>()
                    .0
                    .entry(down_base.clone())
                    .or_default();
                if !entry.bar_visible() || entry.travel <= 0.0 {
                    false
                } else {
                    let ry = f32::from(event.position.y) - entry.bar_top;
                    let top = entry.thumb_top();
                    let grab = if ry >= top && ry <= top + entry.thumb_h {
                        ry - top
                    } else {
                        entry.thumb_h * 0.5
                    };
                    entry.drag_grab = Some(grab);
                    entry.bar_hover = true;
                    entry.target = ((ry - grab) / entry.travel).clamp(0.0, 1.0) * entry.max;
                    true
                }
            };
            if reveal {
                reveal_bar(name, window, cx);
                window.refresh();
            }
        })
        // 进入拖动：标记 dragging（偏移跟手 + 保证显形）。
        .on_drag(
            ScrollbarDrag,
            move |_value: &ScrollbarDrag, _offset, window, cx| {
                let entry = cx
                    .global_mut::<Registry>()
                    .0
                    .entry(drag_base.clone())
                    .or_default();
                entry.dragging = true;
                reveal_bar(name, window, cx);
                cx.new(|_| ScrollbarPreview)
            },
        )
        .on_mouse_up(MouseButton::Left, move |_event, window, cx| {
            end_drag(name, window, cx);
        })
        .on_mouse_up_out(MouseButton::Left, move |_event, window, cx| {
            end_drag(name, window, cx);
        })
        // 指针离开窗口时强制收回（否则离开瞬间光标停在滚动条上会一直常驻）。
        .on_mouse_exit(move |_event, window, cx| {
            let entry = cx
                .global_mut::<Registry>()
                .0
                .entry(exit_base.clone())
                .or_default();
            entry.bar_hover = false;
            entry.dragging = false;
            entry.drag_grab = None;
            window.refresh();
        })
        .child(thumb)
}

/// 按滚轮增量滚动某个滚动区（供页面空白区把滚轮转发给列表用）。
///
/// 只改目标偏移：边界由注册表里的 `max` 收口，动画、滚动条与重绘由该滚动区自己的渲染接管。
pub(crate) fn smooth_scroll_by(name: &'static str, dy: f32, window: &mut Window, cx: &mut App) {
    {
        let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
        if entry.max <= 0.0 {
            log::debug!("[{name}] 滚轮转发被忽略：max={:.0}（内容未量出或不足一屏）", entry.max);
            return;
        }
        let before = entry.target;
        entry.target = (entry.target - dy).clamp(0.0, entry.max);
        log::debug!(
            "[{name}] 滚轮转发 dy={dy:.1}：target {before:.0} → {:.0}（max {:.0}）",
            entry.target,
            entry.max
        );
    }
    reveal_bar(name, window, cx);
    window.refresh();
}

/// 拖动滚动条：指针在任意位置都持续更新偏移（drag 系统的 `on_drag_move` 是全局的）。
fn on_bar_drag_move(name: &'static str, event: &DragMoveEvent<ScrollbarDrag>, window: &mut Window, cx: &mut App) {
    let ry = f32::from(event.event.position.y - event.bounds.origin.y);
    {
        let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
        let Some(grab) = entry.drag_grab else {
            return;
        };
        if entry.travel <= 0.0 {
            return;
        }
        entry.dragging = true;
        entry.target = ((ry - grab) / entry.travel).clamp(0.0, 1.0) * entry.max;
    }
    reveal_bar(name, window, cx);
    window.refresh();
    cx.stop_propagation();
}

/// 处理滚轮：累积目标偏移、置「驻留中」并（重新）安排驻留结束。
fn on_wheel(name: &'static str, event: &ScrollWheelEvent, window: &mut Window, cx: &mut App) {
    let delta = event.delta.pixel_delta(window.line_height());
    let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
    // 触控板横向 pan：本容器只有纵向滚动，横向不吞，透传给内层/外层滚动区。
    if dx.abs() > dy.abs() {
        return;
    }
    cx.stop_propagation();
    {
        let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
        let before = entry.target;
        entry.target = (entry.target - dy).clamp(0.0, entry.max);
        if (entry.target - before).abs() > 0.01 {
            log::debug!(
                "[{name}] 滚轮 dy={dy:.1}：target {before:.0} → {:.0}（max {:.0}）",
                entry.target,
                entry.max
            );
        }
    }
    reveal_bar(name, window, cx);
    window.refresh();
}

/// 设定某滚动区的目标偏移（内容坐标像素）：打开浮层定位到当前曲等场景调用。
///
/// 内容尚未量出（max=0）时先记入 `pending`，待首帧内容高度量出后恢复并钳制（N5.6：
/// 首帧滚轮/定位不被「max=0 时 clamp 归 0」吞掉）。
pub(crate) fn smooth_scroll_to(name: &'static str, target: f32, cx: &mut App) {
    ensure_registry(cx);
    let entry = cx.global_mut::<Registry>().0.entry(entry_id(name)).or_default();
    if entry.max > 0.0 {
        entry.target = target.clamp(0.0, entry.max);
        entry.pending = None;
    } else {
        entry.pending = Some(target);
    }
}

/// 由视口/内容高算滑块长度与可移动距离。
fn thumb_geometry(viewport_h: f32, content_h: f32) -> (f32, f32) {
    if content_h <= 0.0 || viewport_h <= 0.0 {
        return (viewport_h, 0.0);
    }
    let thumb_h = (viewport_h * viewport_h / content_h).clamp(MIN_THUMB.min(viewport_h), viewport_h);
    (thumb_h, (viewport_h - thumb_h).max(0.0))
}

/// 容器上通用的滚动交互接线。
fn scroll_container(name: &'static str, id: ElementId, body: AnyElement, bar: Option<AnyElement>) -> impl IntoElement {
    let viewport_id = id.clone();
    div()
        .id(id)
        .relative()
        .flex_1()
        .w_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_hidden()
        .on_scroll_wheel(move |event, window, cx| on_wheel(name, event, window, cx))
        .on_drag_move::<ScrollbarDrag>(move |event, window, cx| on_bar_drag_move(name, event, window, cx))
        .on_mouse_up(MouseButton::Left, move |_event, window, cx| end_drag(name, window, cx))
        .on_mouse_up_out(MouseButton::Left, move |_event, window, cx| end_drag(name, window, cx))
        .on_prepaint(move |bounds, _window, cx| {
            let h = f32::from(bounds.size.height);
            cx.global_mut::<Registry>()
                .0
                .entry(viewport_id.clone())
                .or_default()
                .viewport = h;
        })
        .child(body)
        .when_some(bar, |el, bar| el.child(bar))
}

/// 把 `content` 包成平滑滚动容器（默认带右缘细滚动条）。
pub(crate) fn smooth_scroll(
    name: &'static str,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    smooth_scroll_with(name, content, true, window, cx)
}

/// 同 [`smooth_scroll`]，但不显示滚动条（仍有平滑滚动）。
pub(crate) fn smooth_scroll_plain(
    name: &'static str,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    smooth_scroll_with(name, content, false, window, cx)
}

fn smooth_scroll_with(
    name: &'static str,
    content: impl IntoElement,
    show_bar: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = entry_id(name);
    ensure_registry(cx);
    let (target, content_h, viewport_h, dragging) = cx
        .global::<Registry>()
        .0
        .get(&id)
        .map(|entry| (entry.target, entry.content, entry.viewport, entry.dragging))
        .unwrap_or((0.0, 0.0, 0.0, false));
    let max = (content_h - viewport_h).max(0.0);
    {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.max = max;
        // max 从 0 变大（首帧内容量出）：恢复挂起的目标，再按当前量程钳制并写回。
        if max > 0.0 {
            if let Some(pending) = entry.pending.take() {
                entry.target = pending;
            }
        }
        entry.target = entry.target.clamp(0.0, max);
    }
    let ease = if dragging { Duration::ZERO } else { SCROLL_EASE };
    let offset = transition(
        (id.clone(), "scroll"),
        target.clamp(0.0, max),
        Transition::new(ease).ease(motion::ease_out()),
        window,
        cx,
    );
    let mouse = window.mouse_position();
    let active = window.is_window_active();
    let (bar_visible, thumb_top, thumb_h) = {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.offset = offset;
        let (thumb_h, travel) = thumb_geometry(viewport_h, content_h);
        entry.thumb_h = thumb_h;
        entry.travel = travel;
        // 以真实光标位置为准（离开窗口后不再更新，故结合窗口是否激活）。
        entry.bar_hover = entry.dragging || (active && entry.hits_bar(mouse));
        (entry.bar_visible(), entry.thumb_top(), thumb_h)
    };
    let opacity = transition(
        (id.clone(), "bar"),
        if bar_visible { 1.0 } else { 0.0 },
        Transition::new(BAR_FADE).ease(motion::ease_out()),
        window,
        cx,
    );

    let content_id = id.clone();
    let body = div()
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
        .child(content)
        .into_any_element();
    let bar = (show_bar && max > 0.5).then(|| scroll_bar(name, thumb_h, thumb_top, opacity).into_any_element());
    scroll_container(name, id, body, bar)
}

/// 可平滑滚动的**虚拟列表**：只渲染可见行，偏移由自己维护（因此能套 `transition` 缓动）。
///
/// 用于替代 `uniform_list`（后者自管偏移、无法被驱动）。`row_height` 为等高行高；
/// `render_range` 与 `uniform_list` 的闭包一致，返回该范围的行元素。
pub(crate) fn smooth_virtual_list(
    name: &'static str,
    item_count: usize,
    row_height: f32,
    render_range: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = entry_id(name);
    ensure_registry(cx);
    let content_h = item_count as f32 * row_height;
    let (viewport_h, dragging, growth_for_layout, growth_now) =
        cx.global::<Registry>()
            .0
            .get(&id)
            .map_or((0.0, false, 0.0, 0.0), |entry| {
                (
                    entry.viewport,
                    entry.dragging,
                    entry.growth_for_layout,
                    entry.header_growth,
                )
            });
    let max = (content_h - viewport_h).max(0.0);
    // 展开态量程：视口与收起量同帧配对，因此与当前折叠进度无关（折叠决策据此）。
    let max_expanded = (content_h - (viewport_h - growth_for_layout)).max(0.0);
    let target = {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.content = content_h;
        entry.max = max;
        entry.max_expanded = max_expanded;
        entry.growth_for_layout = growth_now;
        // 写回，避免列表缩短后 target 停留在越界值、视口瞬间跳回旧位置。
        entry.target = entry.target.clamp(0.0, max);
        entry.target
    };
    let ease = if dragging { Duration::ZERO } else { SCROLL_EASE };
    let offset = transition(
        (id.clone(), "scroll"),
        target,
        Transition::new(ease).ease(motion::ease_out()),
        window,
        cx,
    );

    // 仅渲染可见范围（首行 + 视口内行数，多预读 2 行）。
    let first = (offset / row_height).floor().max(0.0) as usize;
    let visible = (viewport_h / row_height).ceil() as usize + 2;
    let last = (first + visible).min(item_count);
    let rows = if item_count == 0 || first >= last {
        Vec::new()
    } else {
        render_range(first..last, window, cx)
    };
    let body = div()
        .absolute()
        .top(px(first as f32 * row_height - offset))
        .left(px(0.0))
        .right(px(0.0))
        .v_flex()
        .children(rows)
        .into_any_element();

    let mouse = window.mouse_position();
    let active = window.is_window_active();
    let (bar_visible, thumb_top, thumb_h) = {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.offset = offset;
        // ── 诊断日志：布局（行数/视口/量程/目标）变化时才输出，滚动本身不刷屏 ──
        let snap = (item_count, viewport_h as u32, max as u32, target as u32);
        if entry.diag_last != Some(snap) {
            log::debug!(
                "[{name}] 虚拟列表：{item_count} 行 × {row_height:.0}px = {content_h:.0}；视口 {viewport_h:.0}；max {max:.0}（展开态 {max_expanded:.0}，收起 {growth_for_layout:.0}）；target {target:.0}；offset {offset:.0}；渲染 {first}..{last}"
            );
            entry.diag_last = Some(snap);
        }
        // 量程抖动检测：折叠头由偏移驱动、折叠又改变列表视口，量程会来回变。
        let now = Instant::now();
        match entry.diag_at {
            None => {
                entry.diag_at = Some(now);
                entry.diag_max = max;
            }
            Some(at) if (max - entry.diag_max).abs() > 4.0 => {
                // 只有「方向反复」才算抖动；折叠动画期间量程单调收缩，方向不变。
                let dir = if max > entry.diag_max { 1 } else { -1 };
                if now.duration_since(at) >= Duration::from_secs(1) {
                    entry.diag_flips = 0;
                }
                if entry.diag_dir != 0 && dir != entry.diag_dir {
                    entry.diag_flips = entry.diag_flips.wrapping_add(1);
                }
                entry.diag_dir = dir;
                entry.diag_max = max;
                entry.diag_at = Some(now);
                if entry.diag_flips == 3 {
                    let flips = entry.diag_flips;
                    log::warn!(
                        "[{name}] 滚动量程在 1s 内反复伸缩（方向翻转 {flips} 次，当前 max {max:.0}）。常见根因：折叠头由滚动偏移驱动，折叠又改变列表视口 → 量程来回收缩，底部可能不可达。"
                    );
                }
            }
            Some(_) => {}
        }
        let (thumb_h, travel) = thumb_geometry(viewport_h, content_h);
        entry.thumb_h = thumb_h;
        entry.travel = travel;
        // 以真实光标位置为准（离开窗口后不再更新，故结合窗口是否激活）。
        entry.bar_hover = entry.dragging || (active && entry.hits_bar(mouse));
        (entry.bar_visible(), entry.thumb_top(), thumb_h)
    };
    let opacity = transition(
        (id.clone(), "bar"),
        if bar_visible { 1.0 } else { 0.0 },
        Transition::new(BAR_FADE).ease(motion::ease_out()),
        window,
        cx,
    );
    let bar = (max > 0.5).then(|| scroll_bar(name, thumb_h, thumb_top, opacity).into_any_element());
    scroll_container(name, id, body, bar)
}

#[cfg(test)]
mod tests {
    use super::header_fold_allowed;

    #[test]
    fn fold_gate_requires_collapsed_range_to_clear_action_threshold() {
        // 判据：展开态量程 > 视口增量(296) + 操作栏阈值(80) = 376。
        // 等于 376 时收起后偏移正好是 80，不满足 `> 阈值`，仍会来回振荡 → 禁用。
        assert!(!header_fold_allowed(376.0, 296.0, 80.0));
        assert!(!header_fold_allowed(200.0, 296.0, 80.0));
        assert!(!header_fold_allowed(0.0, 296.0, 80.0));
        assert!(header_fold_allowed(376.5, 296.0, 80.0));
        assert!(header_fold_allowed(1000.0, 296.0, 80.0));
    }
}
