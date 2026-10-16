//! 界面主题：颜色与尺寸（对齐 Melodia 桌面端的 `DesktopTheme.kt`）。
//!
//! 这里是**产品主题的唯一定义处**（允许出现原始颜色值），并提供 [`install`]
//! 把调色板注入 gpui-kit 的语义主题，使内置组件（Button/Slider/Input 等）
//! 与自定义界面保持一致。

use gpui_kit::component::Theme;
use gpui_kit::{linear_color_stop, linear_gradient, rgb, App, Background, Rgba};

pub fn window_bg() -> Rgba {
    rgb(0x000000)
}
pub fn pane() -> Rgba {
    rgb(0x121212)
}
pub fn surface() -> Rgba {
    rgb(0x2A2A2A)
}
pub fn card_surface() -> Rgba {
    rgb(0x1F1F1F)
}
pub fn surface_light() -> Rgba {
    rgb(0x3E3E3E)
}
pub fn text_primary() -> Rgba {
    rgb(0xFFFFFF)
}
pub fn text_gray() -> Rgba {
    rgb(0xB3B3B3)
}
pub fn accent() -> Rgba {
    rgb(0xC20C0C)
}
pub fn cover_placeholder() -> Rgba {
    rgb(0x2C2C2C)
}

pub const PLAYER_BAR_H: f32 = 80.0;

/// 降低亮度（用于封面取色背景）。
pub fn darken(color: Rgba, factor: f32) -> Rgba {
    Rgba {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: 1.0,
    }
}

/// 由封面主色生成的「顶部取色背景」：上深下淡，垂直渐变融入面板底色。
///
/// 用于歌单/专辑详情页顶部 hero（对齐 Melodia 的封面回衬），无主色时调用方回退纯色。
pub fn cover_backdrop(color: Rgba) -> Background {
    linear_gradient(
        180.0,
        linear_color_stop(darken(color, 0.42), 0.0),
        linear_color_stop(pane(), 1.0),
    )
}

/// 把 Melodia 调色板注入 gpui-kit 语义主题。
///
/// 之后内置组件（Button 主色、Slider 轨道/滑块、Input 底色、菜单、滚动条等）
/// 会采用同一套颜色，与自定义界面统一。应在 `Theme::change(Dark)` 之后调用。
pub fn install(cx: &mut App) {
    Theme::update(cx, |theme| {
        let c = &mut theme.colors;
        c.background = rgb(0x000000).into();
        c.foreground = rgb(0xFFFFFF).into();
        c.border = rgb(0x2A2A2A).into();
        c.input = rgb(0x2A2A2A).into();
        c.muted = rgb(0x1F1F1F).into();
        c.muted_foreground = rgb(0xB3B3B3).into();
        c.secondary = rgb(0x2A2A2A).into();
        c.secondary_foreground = rgb(0xFFFFFF).into();
        c.secondary_hover = rgb(0x3E3E3E).into();
        // 按钮
        c.button = rgb(0x2A2A2A).into();
        c.button_foreground = rgb(0xFFFFFF).into();
        c.button_hover = rgb(0x3E3E3E).into();
        c.button_active = rgb(0x3E3E3E).into();
        c.button_primary = rgb(0xC20C0C).into();
        c.button_primary_foreground = rgb(0xFFFFFF).into();
        c.button_primary_hover = rgb(0xD01616).into();
        c.button_primary_active = rgb(0xA00A0A).into();
        c.button_secondary = rgb(0x2A2A2A).into();
        c.button_secondary_foreground = rgb(0xFFFFFF).into();
        c.button_secondary_hover = rgb(0x3E3E3E).into();
        c.popover = rgb(0x282828).into();
        c.popover_foreground = rgb(0xFFFFFF).into();
        // 主色 / 强调色 → Melodia 红
        c.primary = rgb(0xC20C0C).into();
        c.primary_foreground = rgb(0xFFFFFF).into();
        c.primary_hover = rgb(0xD01616).into();
        c.primary_active = rgb(0xA00A0A).into();
        c.accent = rgb(0xC20C0C).into();
        c.accent_foreground = rgb(0xFFFFFF).into();
        c.ring = rgb(0xC20C0C).into();
        c.selection = rgb(0xC20C0C).into();
        // 侧栏 / 播放控件
        c.sidebar = rgb(0x121212).into();
        c.sidebar_foreground = rgb(0xFFFFFF).into();
        c.slider_bar = rgb(0x3E3E3E).into();
        c.slider_thumb = rgb(0xFFFFFF).into();
        c.progress_bar = rgb(0x3E3E3E).into();
        c.scrollbar = rgb(0x000000).into();
        c.scrollbar_thumb = rgb(0x3E3E3E).into();
        c.title_bar = rgb(0x000000).into();
        c.window_border = rgb(0x2A2A2A).into();
        theme.radius = gpui_kit::px(8.0);
        theme.radius_lg = gpui_kit::px(10.0);
    });
}
