//! # siren-app
//!
//! 应用层：GPUI 界面与状态管理。
//!
//! ## 约束
//!
//! **本 crate 不得出现 `#[cfg(target_os)]`** —— 平台差异收敛在 `siren-platform`。
//!
//! 详见 `docs/01-架构设计.md`。

pub mod theme;
pub mod ui;

use gpui_kit::*;

use ui::Root;

/// 应用入口：初始化 tokio 运行时与 GPUI 主循环。
pub fn run() {
    let runtime = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let ox = env_f32("SIREN_X", 80.0);
    let oy = env_f32("SIREN_Y", 80.0);
    let w = env_f32("SIREN_W", 1100.0);
    let h = env_f32("SIREN_H", 760.0);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            crate::theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(ox), px(oy)),
                        size: size(px(w), px(h)),
                    })),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Siren".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                move |window, cx| Root::new(window, cx, runtime),
            )
            .expect("打开窗口失败");
        });
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}
