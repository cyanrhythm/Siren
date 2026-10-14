//! 拖动动效的共享时长与缓动曲线（对齐 `docs/09-动效制作指南.md`）。
//!
//! 所有动画都走 `gpui-base` 的 `with_animation`，其内部自动尊重
//! `App::reduce_motion`（系统「减少动态效果」时渲染终态、不排帧），无需另行处理。

use std::time::Duration;

use gpui_kit::base::animation::cubic_bezier;

/// 拖动拾起：浮动预览的入场时长。进入用强减速（快起慢停）。
pub(crate) const DRAG_ENTER: Duration = Duration::from_millis(160);

/// 重排归位：行在屏幕内滑动到新槽位的时长。移动用强加减速。
pub(crate) const REORDER: Duration = Duration::from_millis(170);

/// 强 ease-out：`cubic-bezier(0.23, 1, 0.32, 1)` —— UI 入场 / 退出。
pub(crate) fn ease_out() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.23, 1.0, 0.32, 1.0)
}

/// 强 ease-in-out：`cubic-bezier(0.77, 0, 0.175, 1)` —— 屏幕内位移 / 变形。
pub(crate) fn ease_in_out() -> impl Fn(f32) -> f32 + 'static {
    cubic_bezier(0.77, 0.0, 0.175, 1.0)
}
