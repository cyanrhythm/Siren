//! 界面主题：颜色与尺寸（对齐 Melodia 桌面端的 `DesktopTheme.kt`）。
//!
//! 这里是**产品主题的唯一定义处**（允许出现原始颜色值），并提供 [`install`]
//! 把调色板注入 gpui-kit 的语义主题，使内置组件（Button/Slider/Input 等）
//! 与自定义界面保持一致。

use gpui_kit::component::Theme;
use gpui_kit::{linear_color_stop, linear_gradient, rgb, rgba, App, Background, Rgba};

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
/// 图标键的**按下**态底：比 hover 略亮。
///
/// `ButtonCustomVariant` 的 `active` 默认为透明，不显式指定就会"按下即失去底色"
/// （整改清单 O1.10）。
pub fn surface_pressed() -> Rgba {
    rgb(0x4A4A4A)
}
/// 白色实心控件（播放键）的 hover / 按下态：逐级**轻微**压暗。
///
/// 静止底色由调用方 `.bg(text_primary())` 直给（纯白）：**不能**用
/// `ButtonCustomVariant::color(...)`——kit 会把 Custom 变体的静止色
/// `mix_oklab(transparent, 0.2)`，纯白被压成 `#E8E8E8`，看着就是个灰按钮；
/// 而 hover / active 是**原样取用**，于是「默认灰、hover 白」的错觉就出现了。
/// 这两个值只需比纯白略深，既给到反馈，又不会让深色图标失去对比。
pub fn solid_hover() -> Rgba {
    rgb(0xE6E6E6)
}
pub fn solid_active() -> Rgba {
    rgb(0xCCCCCC)
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

// ---- 尺寸 token（设计规范：间距/圆角/字号收敛到命名档位，不散落魔法数字） ----

/// 间距语义档（px）：2/4/8/12/16/24/32，用法 `.gap(px(spacing::SM))`。
pub mod spacing {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// 圆角语义档：控件 sm/md、浮层 lg；pill 形一律 `rounded_full()`，勿手造值。
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;

/// 字号档：hero 大标题（歌单名等）。
pub mod text {
    pub const HERO: f32 = 40.0;
}

/// 浮层遮罩（modal 底层压暗）。
pub fn scrim() -> Rgba {
    rgba(0x00000088)
}
/// 封面裁剪等需要更强压暗的遮罩。
pub fn scrim_strong() -> Rgba {
    rgba(0x000000cc)
}

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
        // 聚焦态只保留 1px 的 ring 色描边（输入框/按钮自带的 `border_1`）。
        // 开启时 kit 会在边框外再叠一圈 3px、50% 透明的 ring 色（`FOCUS_RING_WIDTH`
        // / `FOCUS_RING_OPACITY`），视觉上是一圈发虚的红晕——与本项目的扁平风格不符。
        theme.focus_ring = false;
    });
}
