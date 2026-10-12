//! 界面主题：颜色与尺寸（对齐 Melodia 桌面端的 `DesktopTheme.kt`）。

use gpui_kit::{rgb, Rgba};

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
