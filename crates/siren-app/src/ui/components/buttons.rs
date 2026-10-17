//! 可复用按钮：红色圆形播放键、图标键。
//!
//! 这两类在页面里出现过十几次，形状（尺寸 + 圆角 + 底色 + hover + 图标）逐字重复，
//! 只有回调不同，故抽到这里。**元素 id 由调用方给**：同一 view 内不能重名
//! （见 `docs/10-状态管理约定.md` §7，重复 id 会触发 gpui 的 a11y 断言）。

use gpui_kit::assets::IconName;
use gpui_kit::*;

use super::super::icon;
use crate::theme;

/// 红色圆形播放键（品牌强调色底 + 播放三角）。
///
/// `size` 是直径，`icon_size` 是三角尺寸——两处历史取值不同（48→22、56→26），
/// 也并非固定的比例，所以显式传，不按直径推算。
pub(crate) fn play_circle(
    id: impl Into<ElementId>,
    size: f32,
    icon_size: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::accent())
        .cursor_pointer()
        .on_click(on_click)
        .child(icon(IconName::Play, icon_size, theme::text_primary()))
        .into_any_element()
}

/// 图标键：悬停给一层 `surface` 底色。
///
/// `hit` 是命中区边长、`radius` 是圆角——页头的圆形键传 `hit / 2`（等价 `rounded_full`），
/// 面板头（播放队列的垃圾桶/关闭）用 `RADIUS_SM`，两者只差这一个数，故不再分两个函数。
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    name: IconName,
    icon_size: f32,
    color: Rgba,
    hit: f32,
    radius: f32,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .size(px(hit))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface()))
        .on_click(on_click)
        .child(icon(name, icon_size, color))
        .into_any_element()
}
