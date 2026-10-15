//! 系统托盘（跨平台：`tray-icon` crate）。
//!
//! 三平台统一：Windows `Shell_NotifyIcon` / Linux StatusNotifierItem（ksni，纯 D-Bus）/
//! macOS `NSStatusItem`。平台差异由 crate 与构建配置吸收（Linux 经 `ksni` feature，
//! 见 `docs/02-平台能力规格.md` §9），本文件为**跨平台实现**，无平台分支。
//!
//! 交互：左键切换主窗口显隐；右键菜单（显示/隐藏、退出）。
//! 事件经全局接收器轮询，由上层在主循环里取用（[`poll_tray_events`]）。

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// 托盘交互事件（平台无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// 左键单击托盘图标：切换主窗口显隐。
    ToggleWindow,
    /// 菜单「退出」。
    Quit,
}

/// 托盘句柄（需保活；丢弃即移除托盘图标）。
pub struct Tray {
    _icon: TrayIcon,
    toggle_id: MenuId,
    quit_id: MenuId,
}

/// 创建托盘；失败返回 `None`（如无图标 / 无托盘宿主）。
pub fn create_tray() -> Option<Tray> {
    let icon = load_icon()?;
    let toggle = MenuItem::new("显示 / 隐藏主窗口", true, None);
    let quit = MenuItem::new("退出 Siren", true, None);
    let toggle_id = toggle.id().clone();
    let quit_id = quit.id().clone();
    let menu = Menu::new();
    menu.append_items(&[&toggle, &quit]).ok()?;

    let tray = TrayIconBuilder::new()
        .with_tooltip("Siren")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        // 左键单击用于切换窗口显隐，菜单仅右键弹出。
        .with_menu_on_left_click(false)
        .build()
        .ok()?;

    Some(Tray {
        _icon: tray,
        toggle_id,
        quit_id,
    })
}

/// 非阻塞轮询托盘与菜单事件，追加到 `out`。
pub fn poll_tray_events(tray: &Tray, out: &mut Vec<TrayEvent>) {
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            out.push(TrayEvent::ToggleWindow);
        }
    }
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        if event.id == tray.toggle_id {
            out.push(TrayEvent::ToggleWindow);
        } else if event.id == tray.quit_id {
            out.push(TrayEvent::Quit);
        }
    }
}

/// 从内嵌 PNG 解码托盘图标。
fn load_icon() -> Option<Icon> {
    const BYTES: &[u8] = include_bytes!("../../../assets/icons/tray.png");
    let mut decoder = png::Decoder::new(BYTES);
    decoder.set_transformations(png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba = buf[..info.buffer_size()].to_vec();
    Icon::from_rgba(rgba, info.width, info.height).ok()
}
