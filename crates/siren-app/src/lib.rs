//! # siren-app
//!
//! 应用层：GPUI 界面与状态管理。
//!
//! ## 约束
//!
//! **本 crate 不得出现 `#[cfg(target_os)]`** —— 平台差异收敛在 `siren-platform`。
//!
//! 详见 `docs/01-架构设计.md`。

// `gpui_kit::*` 通配导入下展开 `#[test]` 会触及默认递归深度上限。
#![recursion_limit = "256"]

mod disk_lru;
mod lyrics_cache;
mod meta_cache;
pub mod theme;
pub mod ui;

use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::*;

use ui::Root;

/// 内嵌的应用字体（MiSans）。
const MI_SANS: &[u8] = include_bytes!("../../../assets/fonts/MiSans-Regular.ttf");

// 默认宽不得小于最小窗口宽（min_window_size 按侧栏 + 双卡片 + 右栏计算，见 ui/mod.rs）。
const DEFAULT_W: f32 = 1200.0;
const DEFAULT_H: f32 = 760.0;

/// CSD（客户端装饰）每边的阴影内边距，窗口最小尺寸作用于外沿时需计入
/// （gpui-component `window_border` 在 Linux 的 `SHADOW_SIZE` = 20px/边）。
const CSD_FRAME: f32 = 20.0;

thread_local! {
    /// 托盘句柄（保活；须与创建它的主线程同处）。
    static TRAY: RefCell<Option<siren_platform::PlatformTray>> = const { RefCell::new(None) };
}
/// 托盘是否就绪（决定「关闭到托盘」是否生效）。
static TRAY_READY: AtomicBool = AtomicBool::new(false);
/// 是否正在退出（退出时关闭窗口不再拦为托盘最小化）。
static QUITTING: AtomicBool = AtomicBool::new(false);

/// 应用入口：初始化 tokio 运行时与 GPUI 主循环。
pub fn run() {
    // 尽早初始化日志（文件 + 可选标准错误镜像）
    siren_core::logging::init();

    // 注册 AppUserModelID：让系统媒体浮层显示应用名而非“未知应用”
    siren_platform::set_app_user_model_id("com.cyanrhythm.Siren");

    // 单实例：已有实例则唤起其窗口并退出本进程（避免多开）。
    if !siren_platform::ensure_single_instance("com.cyanrhythm.Siren") {
        log::info!("检测到已有实例，已唤起其窗口；本进程退出");
        return;
    }

    // Runtime 由本函数持有（`run` 阻塞在事件循环，直到应用退出才返回），只把 Handle 交给 UI，
    // 避免根视图持有一条完整 Runtime 掩盖真实依赖（F1）。
    let runtime = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let rt_handle = runtime.handle().clone();

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

    // 最小窗口尺寸：容纳「左栏 + 中间两个卡片 + 右栏」，高度取宽度的 0.618。
    // CSD（Linux）下最小尺寸作用于窗口外沿（含阴影内边距），需额外计入两周的 padding。
    let (min_w, min_h) = ui::min_window_size(siren_core::settings::Settings::load().np_width());
    let csd_frame = if siren_platform::prefers_client_decorations() {
        CSD_FRAME * 2.0
    } else {
        0.0
    };
    let (min_w, min_h) = (min_w + csd_frame, min_h + csd_frame);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            // 注册应用字体并作为默认字体系
            if cx.text_system().add_fonts(vec![Cow::Borrowed(MI_SANS)]).is_ok() {
                log::info!("已注册字体 MiSans");
            }
            gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
            cx.global_mut::<gpui_kit::component::Theme>().font_family = "MiSans".into();
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
                    // Linux：请求客户端装饰（自绘标题栏）；否则合成器会再画一套（双标题栏）。
                    // X11 无合成器时 GPUI 自动回退服务端装饰，UI 侧据实际模式降级。
                    window_decorations: siren_platform::prefers_client_decorations()
                        .then_some(WindowDecorations::Client),
                    // Linux（Wayland/X11）：桌面环境据 app_id 匹配 .desktop（图标/分组/激活）
                    app_id: Some("com.cyanrhythm.Siren".into()),
                    // 先隐藏创建，移动到目标位置后再显示，避免可见的跳动
                    show: false,
                    focus: true,
                    window_min_size: Some(size(px(min_w), px(min_h))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    // 关闭：默认最小化到托盘（托盘可用且设置开启时）；否则真正退出。
                    window.on_window_should_close(cx, request_close);
                    // 恢复上次物理矩形 / 首次居中：延迟到窗口建立后再移动，避免渲染期重入
                    let target = if let Some((x, y, w, h)) = saved {
                        Some(clamp_visible((x as i32, y as i32, w as i32, h as i32)))
                    } else if centered {
                        siren_platform::primary_work_area_physical().map(|(wx, wy, ww, wh)| {
                            let scale = window.scale_factor();
                            let (pw, ph) = ((DEFAULT_W * scale) as i32, (DEFAULT_H * scale) as i32);
                            (wx + (ww - pw) / 2, wy + (wh - ph) / 2, pw, ph)
                        })
                    } else {
                        None
                    };
                    // 可跨 await 持有的窗口句柄；窗口已关闭时 update 失败，忽略即可。
                    let window_handle = window.window_handle();
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(60))
                            .await;
                        // 只在 update 内「捕获」窗口（无副作用、不接触原生句柄）；窗口操作放到
                        // update **之外**再应用：否则 `SetWindowPos` 会同步派发 WM_MOVE 重入 gpui
                        // 的窗口过程，其消息处理再次借用 App → "RefCell already borrowed" panic。
                        let win = window_handle
                            .update(cx, |_view, window, _cx| siren_platform::capture_window(window))
                            .ok()
                            .flatten();
                        let Some(win) = win else {
                            return;
                        };
                        if let Some(t) = target {
                            if !win.move_to(t.0, t.1, t.2, t.3) {
                                // 平台无法定位（Wayland：位置由合成器决定）：至少恢复上次尺寸
                                //（GPUI 操作，放回 update 内执行）。
                                let _ = window_handle.update(cx, |_view, window, _cx| {
                                    window.resize(size(px(t.2 as f32), px(t.3 as f32)));
                                });
                            }
                        }
                        let icon_ok = win.set_icon();
                        log::info!("set_window_icon -> {icon_ok}");
                        win.show();
                    })
                    .detach();
                    // 系统托盘（左键切换显隐 / 菜单退出）+ 平台事件轮询（单实例激活）。
                    setup_tray();
                    spawn_platform_event_poll(window_handle, cx);
                    Root::new(window, cx, rt_handle.clone())
                },
            )
            .expect("打开窗口失败");
        });
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// 恢复的窗口矩形若完全落在可见区（所有显示器合成的虚拟屏幕）之外——如显示器拔除/改分辨率——，
/// 则居中到主显示器工作区，避免窗口不可见。
fn clamp_visible(rect: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = rect;
    let on_screen = siren_platform::virtual_screen_physical()
        .map(|(vx, vy, vw, vh)| x < vx + vw && x + w > vx && y < vy + vh && y + h > vy)
        .unwrap_or(true);
    if on_screen {
        return rect;
    }
    match siren_platform::primary_work_area_physical() {
        Some((wx, wy, ww, wh)) => {
            let nw = w.min(ww);
            let nh = h.min(wh);
            (wx + (ww - nw) / 2, wy + (wh - nh) / 2, nw, nh)
        }
        None => rect,
    }
}

/// 统一关闭处理（系统关闭事件与自绘关闭按钮共用）。
///
/// 托盘可用且开启「关闭到托盘」时：隐藏窗口（平台不支持隐藏时降级为最小化，
/// 如 Wayland——见 `siren_platform::window_system` 的降级契约），进程留在托盘，
/// 返回 `false`；否则同步落盘后返回 `true`（调用方真正关闭窗口）。
pub(crate) fn request_close(window: &mut Window, cx: &mut App) -> bool {
    if !QUITTING.load(Ordering::SeqCst) && TRAY_READY.load(Ordering::SeqCst) && ui::close_to_tray(cx) {
        if !siren_platform::hide_window(window) {
            // 平台无原生隐藏能力（Wayland 无显隐协议）：降级为最小化，托盘仍可召回。
            window.minimize_window();
        }
        return false;
    }
    // 真正退出：同步落盘播放会话（队列/当前曲/进度）+ 保存窗口位置/大小
    ui::flush_playback_session(cx);
    save_window_bounds_now(window);
    true
}

/// 保存窗口位置/大小：优先平台原生物理矩形（Windows）；平台不提供几何查询时
/// （Wayland），退化为 GPUI 逻辑坐标——位置由合成器决定，尺寸用于下次恢复。
fn save_window_bounds_now(window: &Window) {
    // 最小化时坐标不可信，跳过（Wayland 下恒为 false）。
    if siren_platform::is_window_minimized(window) {
        return;
    }
    if let Some((x, y, w, h)) = siren_platform::window_physical_rect(window) {
        siren_core::settings::save_window_bounds(x as f32, y as f32, w as f32, h as f32);
        return;
    }
    let bounds = window.window_bounds().get_bounds();
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if w > 0.0 && h > 0.0 {
        siren_core::settings::save_window_bounds(f32::from(bounds.origin.x), f32::from(bounds.origin.y), w, h);
    }
}

/// 初始化系统托盘（左键切换主窗口显隐；菜单「退出」）；失败记为无托盘（关闭即退出）。
fn setup_tray() {
    let Some(tray) = siren_platform::create_tray() else {
        log::warn!("系统托盘创建失败：关闭窗口将直接退出");
        return;
    };
    TRAY.with(|cell| *cell.borrow_mut() = Some(tray));
    TRAY_READY.store(true, Ordering::SeqCst);
}

/// 轮询平台事件：托盘交互（左键切换显隐 / 菜单退出）+ 单实例激活请求。
///
/// 句柄层无法执行的窗口操作（Wayland：显隐/几何无协议）在此用 GPUI 窗口 API 兜底，
/// 见 `siren_platform::window_system` 的降级契约。
fn spawn_platform_event_poll(window_handle: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx| loop {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(120))
            .await;
        let mut tray_events = Vec::new();
        TRAY.with(|cell| {
            if let Some(tray) = cell.borrow().as_ref() {
                siren_platform::poll_tray_events(tray, &mut tray_events);
            }
        });
        let mut activation_events = Vec::new();
        siren_platform::poll_activation_events(&mut activation_events);
        for event in tray_events {
            match event {
                siren_platform::TrayEvent::ToggleWindow => {
                    // 捕获后拖到 update 之外执行原生显隐：SetForegroundWindow 同样可能同步重入。
                    let win = window_handle
                        .update(cx, |_view, window, _cx| siren_platform::capture_window(window))
                        .ok()
                        .flatten();
                    if let Some(win) = win {
                        if !win.toggle() {
                            // 平台无原生生显切换（Wayland）：台前则收起，否则唤起。
                            let _ = window_handle.update(cx, |_view, window, _cx| {
                                if window.is_window_active() {
                                    window.minimize_window();
                                } else {
                                    window.activate_window();
                                }
                            });
                        }
                    }
                }
                siren_platform::TrayEvent::Quit => {
                    QUITTING.store(true, Ordering::SeqCst);
                    // 托盘退出走 PostQuitMessage，不触发 WM_CLOSE，故在此保存窗口矩形。
                    let _ = window_handle.update(cx, |_view, window, cx| {
                        save_window_bounds_now(window);
                        ui::flush_playback_session(cx);
                        cx.quit();
                    });
                    return;
                }
            }
        }
        for event in activation_events {
            match event {
                siren_platform::ActivationEvent::ActivateMainWindow => {
                    // 已有实例收到「再次启动」：请 GPUI 经合成器协议唤起窗口
                    // （Wayland 为 xdg_activation，合成器可拒绝，见平台层说明）。
                    log::info!("收到单实例激活请求：唤起主窗口");
                    let _ = window_handle.update(cx, |_view, window, _cx| {
                        window.activate_window();
                    });
                }
            }
        }
    })
    .detach();
}
