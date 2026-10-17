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
    /// 可收起头部的总高度（列表视口在头部完全收起后增加的高度；0 = 无折叠头）。
    header_max: f32,
    /// 当前已收起的头部高度（页面据此设置头部各段高度）。
    collapse: f32,
    /// 与 `viewport` 同帧的收起量：`viewport - layout_collapse` 才是展开态视口。
    layout_collapse: f32,
    /// 总滚动位移 = 头部收起 + 列表滚动；滚动条按它定位。
    scroll: f32,
    /// 视口高（prepaint 量得）。
    viewport: f32,
    /// 滚动条命中区（窗口坐标）：左右上下。
    bar_left: f32,
    bar_top: f32,
    bar_bottom: f32,
    /// 最大可滚偏移（正的像素数）。
    max: f32,
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
    diag_warned: bool,
}

impl Entry {
    fn bar_visible(&self) -> bool {
        self.max > 0.5 && (self.bar_hover || self.scrolling || self.dragging)
    }

    fn thumb_top(&self) -> f32 {
        if self.max > 0.0 {
            (self.scroll / self.max).clamp(0.0, 1.0) * self.travel
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
    virtual_list_impl(
        name,
        item_count,
        row_height,
        HeaderCollapse::default(),
        render_range,
        window,
        cx,
    )
}

/// 带「可收起头部」的虚拟列表。
///
/// `header_max` = 头部完全收起后**列表视口增加的高度**（各段收起量之和）。滚动量程
/// 按「展开态视口」计算，把收起头部当作滚动过程的第一段：先收头（列表不动），
/// 收满后列表才滚。于是 `max` 与折叠进度无关，彻底消除
/// 「折叠改变视口 → 量程变小 → 折叠阈值不可达 → 来回振荡 → 底部不可达」的反馈回路。
///
/// 页面用 [`smooth_scroll_collapse`] 读当前收起量，据此设置头部各段高度（差一帧，不可见）。
pub(crate) fn smooth_virtual_list_with_header(
    name: &'static str,
    item_count: usize,
    row_height: f32,
    header: HeaderCollapse,
    render_range: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    virtual_list_impl(name, item_count, row_height, header, render_range, window, cx)
}

/// 折叠头的两项状态：完全收起后的视口增量、以及当前帧布局使用的收起量。
#[derive(Clone, Copy, Default)]
pub(crate) struct HeaderCollapse {
    /// 头部完全收起后列表视口增加的总高度（0 = 无折叠头）。
    pub max: f32,
    /// 当前帧头部实际收起的高度（页面上一帧从 [`smooth_scroll_collapse`] 读到的值）。
    pub collapse: f32,
}

/// 滚动量程：内容高 −「展开态视口」。`viewport_h` 已随头部收起而变大，减去 `collapse`
/// 才是展开态视口，因此结果与当前折叠进度无关（恒定）。
fn header_scroll_max(content_h: f32, viewport_h: f32, collapse: f32) -> f32 {
    let view_expanded = (viewport_h - collapse).max(0.0);
    (content_h - view_expanded).max(0.0)
}

fn virtual_list_impl(
    name: &'static str,
    item_count: usize,
    row_height: f32,
    header: HeaderCollapse,
    render_range: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = entry_id(name);
    ensure_registry(cx);
    let content_h = item_count as f32 * row_height;
    let (viewport_h, dragging, layout_collapse) =
        cx.global::<Registry>().0.get(&id).map_or((0.0, false, 0.0), |entry| {
            (entry.viewport, entry.dragging, entry.layout_collapse)
        });
    // `viewport` 与 `layout_collapse` 同帧记录：相减得到展开态视口，量程因此恒定。
    let max = header_scroll_max(content_h, viewport_h, layout_collapse);
    let (header_max, collapse) = (header.max, header.collapse);
    let target = {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.content = content_h;
        entry.header_max = header_max;
        entry.max = max;
        // 写回，避免列表缩短后 target 停留在越界值、视口瞬间跳回旧位置。
        entry.target = entry.target.clamp(0.0, max);
        entry.target
    };
    let ease = if dragging { Duration::ZERO } else { SCROLL_EASE };
    // `scroll` 是「头部收起 + 列表滚动」的总位移；`offset` 只是列表自己的偏移。
    let scroll = transition(
        (id.clone(), "scroll"),
        target,
        Transition::new(ease).ease(motion::ease_out()),
        window,
        cx,
    );
    // 本帧的头部布局用的是传入的 `collapse`，列表位移必须与它对齐，否则会差一帧错位。
    let offset = (scroll - collapse).max(0.0);
    // 下一帧页面据此设置头部高度（滚动动画因此逐帧推进）。
    let next_collapse = scroll.clamp(0.0, header_max.max(0.0));

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
    // 滚动条按「头部完全展开时的总内容 / 头部完全收起时的总视口」算：
    // 两者都是常量，滑块长度不会随折叠动画伸缩。
    let total_content = content_h + header_max.max(0.0);
    let total_viewport = viewport_h + (header_max.max(0.0) - collapse).max(0.0);
    let (bar_visible, thumb_top, thumb_h) = {
        let entry = cx.global_mut::<Registry>().0.entry(id.clone()).or_default();
        entry.offset = offset;
        entry.scroll = scroll;
        entry.collapse = next_collapse;
        entry.layout_collapse = collapse;
        // ── 诊断日志：布局（行数/视口/量程/目标/收起量）变化时才输出 ──
        let snap = (item_count, viewport_h as u32, max as u32, target as u32);
        if entry.diag_last != Some(snap) {
            log::debug!(
                "[{name}] 虚拟列表：{item_count} 行 × {row_height:.0}px = {content_h:.0}；视口 {viewport_h:.0}；max {max:.0}；target {target:.0}；scroll {scroll:.0}；收起 {collapse:.0}；offset {offset:.0}；渲染 {first}..{last}"
            );
            entry.diag_last = Some(snap);
        }
        // 量程抖动检测：修复后 max 应恒定；若仍在变，说明另有反馈源。
        let now = Instant::now();
        match entry.diag_at {
            None => {
                entry.diag_at = Some(now);
                entry.diag_max = max;
            }
            Some(at) if (max - entry.diag_max).abs() > 4.0 => {
                // 只有「方向反复」才算抖动；正常内容变化是单调的。
                let dir = if max > entry.diag_max { 1 } else { -1 };
                if now.duration_since(at) >= Duration::from_secs(1) {
                    entry.diag_flips = 0;
                    entry.diag_warned = false;
                }
                if entry.diag_dir != 0 && dir != entry.diag_dir {
                    entry.diag_flips = entry.diag_flips.wrapping_add(1);
                }
                entry.diag_dir = dir;
                entry.diag_max = max;
                entry.diag_at = Some(now);
                if entry.diag_flips == 3 && !entry.diag_warned {
                    entry.diag_warned = true;
                    let flips = entry.diag_flips;
                    log::warn!(
                        "[{name}] 滚动量程在 1s 内反复伸缩（方向翻转 {flips} 次，当前 max {max:.0}）。若折叠头已接入 header_max，说明还有别的反馈源。"
                    );
                }
            }
            Some(_) => {}
        }
        let (thumb_h, travel) = thumb_geometry(total_viewport, total_content);
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

/// 清零某个滚动区（换页/换歌单时调用）：目标、位移、收起量一并归零，不等缓动。
pub(crate) fn smooth_scroll_reset(name: &'static str, cx: &mut App) {
    ensure_registry(cx);
    if let Some(entry) = cx.global_mut::<Registry>().0.get_mut(&entry_id(name)) {
        entry.target = 0.0;
        entry.offset = 0.0;
        entry.scroll = 0.0;
        entry.collapse = 0.0;
        entry.layout_collapse = 0.0;
        entry.pending = None;
        entry.bar_hover = false;
        entry.dragging = false;
        entry.drag_grab = None;
    }
}

/// 当前头部已收起的像素数（未使用折叠头的滚动区返回 0）。
pub(crate) fn smooth_scroll_collapse(name: &'static str, cx: &App) -> f32 {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.0.get(&entry_id(name)).map(|entry| entry.collapse))
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::header_scroll_max;

    #[test]
    fn header_scroll_max_stays_constant_while_header_collapses() {
        let content = 1500.0;
        let view_expanded = 600.0;
        for collapse in [0.0, 40.0, 120.0, 230.0, 296.0] {
            // 头部收起多少，列表视口就长多少。
            let viewport = view_expanded + collapse;
            assert_eq!(
                header_scroll_max(content, viewport, collapse),
                content - view_expanded,
                "collapse={collapse} 时量程不应变化"
            );
        }
    }

    #[test]
    fn short_content_is_revealed_purely_by_collapsing_header() {
        // 内容比展开态视口高 100：量程 100，全部由「收头」吃掉，列表本身不滚。
        let (content, view_expanded) = (700.0, 600.0);
        let max = header_scroll_max(content, view_expanded, 0.0);
        assert_eq!(max, 100.0);
        let collapse = max.min(296.0);
        assert_eq!(view_expanded + collapse, content);
    }

    #[test]
    fn transient_viewport_smaller_than_collapse_is_not_negative() {
        // 视口测量落后于收起量（一帧）时，展开态视口按 0 处理，量程不得为负。
        assert_eq!(header_scroll_max(500.0, 24.0, 296.0), 500.0);
        assert_eq!(header_scroll_max(0.0, 0.0, 0.0), 0.0);
    }
}
