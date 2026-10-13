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

const DEFAULT_W: f32 = 1100.0;
const DEFAULT_H: f32 = 760.0;

/// 应用入口：初始化 tokio 运行时与 GPUI 主循环。
pub fn run() {
    // 尽早初始化日志（文件 + 可选标准错误镜像）
    siren_core::logging::init();

    // 注册 AppUserModelID：让系统媒体浮层显示应用名而非“未知应用”
    siren_platform::set_app_user_model_id("com.cyanrhythm.Siren");

    let runtime = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");

    // 开发期环境变量覆盖；否则用上次保存的窗口位置/大小；都没有则首次居中
    let env_set = ["SIREN_X", "SIREN_Y", "SIREN_W", "SIREN_H"]
        .iter()
        .any(|k| std::env::var(k).is_ok());
    let saved = if env_set {
        None
    } else {
        siren_core::settings::load_window_bounds()
    };
    let centered = !env_set && saved.is_none();
    let (x, y, w, h) = if env_set {
        (
            env_f32("SIREN_X", 80.0),
            env_f32("SIREN_Y", 80.0),
            env_f32("SIREN_W", DEFAULT_W),
            env_f32("SIREN_H", DEFAULT_H),
        )
    } else if let Some(b) = saved {
        b
    } else {
        (80.0, 80.0, DEFAULT_W, DEFAULT_H)
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            crate::theme::install(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(x), px(y)),
                        size: size(px(w), px(h)),
                    })),
                    // 无边框：Windows 后端据 appears_transparent 隐藏系统标题栏
                    titlebar: Some(TitlebarOptions {
                        title: Some("Siren".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    // 先隐藏创建，移动到目标位置后再显示，避免可见的跳动
                    show: false,
                    focus: true,
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    // 关闭时保存窗口的物理矩形，供下次启动跨屏恢复
                    window.on_window_should_close(cx, |window, cx| {
                        // 退出时同步落盘播放会话（队列/当前曲/进度）
                        ui::flush_playback_session(cx);
                        if let Some(hwnd) = siren_platform::window_hwnd(window) {
                            if let Some((x, y, w, h)) = siren_platform::window_physical_rect(hwnd) {
                                siren_core::settings::save_window_bounds(x as f32, y as f32, w as f32, h as f32);
                            }
                        }
                        true
                    });
                    // 恢复上次物理矩形 / 首次居中：延迟到窗口建立后再移动，避免渲染期重入
                    let target = if let Some((x, y, w, h)) = saved {
                        Some((x as i32, y as i32, w as i32, h as i32))
                    } else if centered {
                        siren_platform::primary_work_area_physical().map(|(wx, wy, ww, wh)| {
                            let scale = window.scale_factor();
                            let (pw, ph) = ((DEFAULT_W * scale) as i32, (DEFAULT_H * scale) as i32);
                            (wx + (ww - pw) / 2, wy + (wh - ph) / 2, pw, ph)
                        })
                    } else {
                        None
                    };
                    if let Some(hwnd) = siren_platform::window_hwnd(window) {
                        cx.spawn(async move |cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(60))
                                .await;
                            if let Some(t) = target {
                                siren_platform::move_window(hwnd, t.0, t.1, t.2, t.3);
                            }
                            siren_platform::show_window(hwnd);
                        })
                        .detach();
                    }
                    Root::new(window, cx, runtime)
                },
            )
            .expect("打开窗口失败");
        });
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}
