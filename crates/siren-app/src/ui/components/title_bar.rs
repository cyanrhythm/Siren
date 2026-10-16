//! 标题栏：前进/后退、首页、搜索框、账户、窗口按钮。
//!
//! 对齐 Melodia 桌面端的 `TitleBar.kt`。
//!
//! 拖动区只放在**空白 spacer**上（`WindowControlArea::Drag`）；窗口按钮各自
//! `Min/Max/Close`。若把 Drag 设在整条容器上，父元素命中区会盖住按钮（命中测试
//! 返回第一个匹配的 area），导致最小化/最大化/关闭点击无反应。
//!
//! Windows 下按钮点击由系统按 NC 区域处理（`window_control_area`）；Linux 请求
//! 客户端装饰（CSD）后，最小化/最大化/关闭按钮、空白区拖动（`start_window_move`）
//! 与双击最大化均由这里用 GPUI 窗口 API 执行（见 `siren_platform::window` 的 CSD 说明）。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::Icon;
use gpui_kit::*;

use super::super::session::Session;
use super::super::{icon, PageEvent, Root, Route};
use crate::theme;

impl Root {
    pub(crate) fn title_bar(
        &self,
        weak: WeakEntity<Self>,
        vw: f32,
        client_decorated: bool,
        cx: &App,
    ) -> impl IntoElement {
        let is_home = matches!(self.route(), Route::Home);
        let is_search = matches!(self.route(), Route::Search);

        let weak_back = weak.clone();
        let weak_fwd = weak.clone();
        let weak_home = weak.clone();
        let weak_account = weak.clone();
        // CSD 拖动区（按下置位 / 首次移动交给合成器 / 抬起复位）。
        let weak_drag = weak.clone();
        let weak_search = weak;

        // 账号按钮：已登录显示圆形头像，未登录显示人形占位（对齐 Melodia）。
        // 用 Custom 变体避免 ghost 的 accent(红) hover/active 底，按钮与头像同尺寸的圆形。
        let account = Session::account(cx).cloned();
        let trigger = match &account {
            Some(acc) => Button::new("account")
                .custom(ButtonCustomVariant::new(cx))
                .w(px(32.0))
                .h(px(32.0))
                .p(px(0.0))
                .rounded_full()
                .child(self.services.covers.image_round(&acc.avatar_url, 32.0)),
            None => Button::new("account")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(theme::surface().into())
                        .hover(theme::surface_light().into())
                        .active(theme::surface_light().into())
                        .foreground(theme::text_gray().into()),
                )
                .w(px(32.0))
                .h(px(32.0))
                .p(px(0.0))
                .rounded_full()
                .icon(IconName::User),
        };
        let account_button = trigger.dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            match &account {
                Some(acc) => {
                    let uid = acc.uid;
                    // 头部：仅昵称文本，不可点击
                    menu = menu.label(acc.nickname.clone());
                    menu = menu.separator();
                    let weak = weak_account.clone();
                    menu = menu.item(PopupMenuItem::new("个人主页").icon(Icon::new(IconName::User)).on_click(
                        move |_ev, _window, cx| {
                            weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenProfile { uid }, cx))
                                .ok();
                        },
                    ));
                    let weak = weak_account.clone();
                    menu = menu.item(
                        PopupMenuItem::new("最近播放")
                            .icon(Icon::new(IconName::Clock))
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenRecentPlay, cx))
                                    .ok();
                            }),
                    );
                    let weak = weak_account.clone();
                    menu = menu.item(PopupMenuItem::new("设置").icon(Icon::new(IconName::Settings)).on_click(
                        move |_ev, _window, cx| {
                            weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenSettings, cx))
                                .ok();
                        },
                    ));
                    let weak = weak_account.clone();
                    menu = menu.item(PopupMenuItem::new("账号设置").icon(Icon::new(IconName::User)).on_click(
                        move |_ev, _window, cx| {
                            weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenAccount, cx))
                                .ok();
                        },
                    ));
                    let weak = weak_account.clone();
                    menu.item(
                        PopupMenuItem::new("退出登录")
                            .icon(Icon::new(IconName::LogOut))
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |root, cx| root.logout(cx)).ok();
                            }),
                    )
                }
                None => {
                    let weak = weak_account.clone();
                    menu = menu.item(PopupMenuItem::new("登录").icon(Icon::new(IconName::LogIn)).on_click(
                        move |_ev, _window, cx| {
                            weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenLogin, cx))
                                .ok();
                        },
                    ));
                    let weak = weak_account.clone();
                    menu.item(PopupMenuItem::new("设置").icon(Icon::new(IconName::Settings)).on_click(
                        move |_ev, _window, cx| {
                            weak.update(cx, |root, cx| root.on_page_event(&PageEvent::OpenSettings, cx))
                                .ok();
                        },
                    ))
                }
            }
        });

        // 左：前进 / 后退
        let left = div()
            .w(px(84.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(theme::spacing::XXS))
            .child(
                div()
                    .id("nav-back")
                    .size(px(34.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |root, cx| root.back(cx)).ok();
                    })
                    .child(icon(
                        IconName::ChevronLeft,
                        18.0,
                        if self.can_back() {
                            theme::text_gray()
                        } else {
                            theme::surface_light()
                        },
                    )),
            )
            .child(
                div()
                    .id("nav-fwd")
                    .size(px(34.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_fwd.update(cx, |root, cx| root.forward(cx)).ok();
                    })
                    .child(icon(
                        IconName::ChevronRight,
                        18.0,
                        if self.can_forward() {
                            theme::text_gray()
                        } else {
                            theme::surface_light()
                        },
                    )),
            );

        // 中：首页 + 搜索框。窗口变窄时中段不硬撑 472：搜索框收缩（保最小宽），
        // 左右各留 24px 拖拽区。
        let middle_w = 472.0_f32.min((vw - 84.0 - 182.0 - 48.0).max(0.0));
        let search_w = (middle_w - 52.0).max(120.0);
        let middle = div()
            .w(px(middle_w))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(theme::spacing::SM))
            .child(
                div()
                    .id("home")
                    .size(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    // 选中指示：surface_light 填充（与搜索框同一风格）。
                    .bg(if is_home { theme::surface_light() } else { theme::surface() })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface_light()))
                    .on_click(move |_ev, _window, cx| {
                        weak_home.update(cx, |root, cx| root.navigate(Route::Home, cx)).ok();
                    })
                    .child(icon(IconName::House, 20.0, theme::text_gray())),
            )
            .child(
                div()
                    .id("search-box")
                    .w(px(search_w))
                    .min_w(px(120.0))
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .rounded_full()
                    .bg(if is_search {
                        theme::surface_light()
                    } else {
                        theme::surface()
                    })
                    .on_click(move |_ev, _window, cx| {
                        weak_search.update(cx, |root, cx| root.navigate(Route::Search, cx)).ok();
                    })
                    .child(icon(IconName::Search, 18.0, theme::text_gray()))
                    .child(div()
                            .flex_1()
                            // 去掉输入框自带的边框/背景/聚焦红框，融入外层搜索胶囊。
                            .child(Input::new(&self.search_input).appearance(false))),
            );

        // 右：账户、窗口按钮（下载按钮暂无落地功能，先移除）。
        let right = div()
            .w(px(182.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(theme::spacing::XS))
            .child(account_button)
            .child(window_button(
                "win-min",
                IconName::Minus,
                false,
                WindowControlArea::Min,
                |window, _cx| {
                    window.minimize_window();
                },
            ))
            .child(window_button(
                "win-max",
                IconName::Square,
                false,
                WindowControlArea::Max,
                |window, _cx| {
                    window.zoom_window();
                },
            ))
            .child(window_button(
                "win-close",
                IconName::X,
                true,
                WindowControlArea::Close,
                |window, cx| {
                    if crate::request_close(window, cx) {
                        window.remove_window();
                    }
                },
            ));

        // 仅空白区可拖动（避免命中区盖住按钮）。
        // - Windows（服务端命中测试）：单击落回系统启动拖动；双击时 stop_propagation，
        //   避免系统对 HTCAPTION 的默认最大化。
        // - CSD（Linux 自绘装饰）：按下只置位，**首次移动**才把拖动交给合成器
        //   （对齐 gpui-component `TitleBar`，避免双击的首击误启动移动）；双击最大化/还原。
        let fixed = 84.0 + middle_w + 182.0;
        let sp = ((vw - 24.0 - fixed) / 2.0).max(0.0);
        let drag = |id: &'static str| {
            let weak_down = weak_drag.clone();
            let weak_move = weak_drag.clone();
            let weak_up = weak_drag.clone();
            div()
                .id(id)
                .w(px(sp))
                .h(px(64.0))
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                    if !client_decorated {
                        if ev.click_count == 2 {
                            cx.stop_propagation();
                        }
                        return;
                    }
                    if ev.click_count == 2 {
                        // 双击：清掉首击的待拖动标记，最大化/还原。
                        weak_down.update(cx, |root, _| root.tb_drag_armed = false).ok();
                        window.zoom_window();
                    } else {
                        weak_down.update(cx, |root, _| root.tb_drag_armed = true).ok();
                    }
                })
                .on_mouse_move(move |_ev, window, cx| {
                    if !client_decorated {
                        return;
                    }
                    let armed = weak_move
                        .update(cx, |root, _| std::mem::take(&mut root.tb_drag_armed))
                        .unwrap_or(false);
                    if armed {
                        window.start_window_move();
                    }
                })
                .on_mouse_up(MouseButton::Left, move |_ev, _window, cx| {
                    if client_decorated {
                        weak_up.update(cx, |root, _| root.tb_drag_armed = false).ok();
                    }
                })
        };

        div()
            .id("shell-title-bar")
            .w_full()
            .h(px(64.0))
            .flex()
            .items_center()
            .px(px(theme::spacing::MD))
            .child(left)
            .child(drag("tb-drag-l"))
            .child(middle)
            .child(drag("tb-drag-r"))
            .child(right)
    }
}

/// 窗口控制按钮：最小化 / 最大化 / 关闭。
///
/// - Windows：`window_control_area` 交给系统命中测试（NC 区域），点击由 OS 处理，
///   `on_click` 不会触发（客户端收不到该区域的鼠标消息）。
/// - Linux（Wayland/X11）：平台层无命中测试实现（见 `siren_platform::window_system`
///   的降级契约），点击由 `on_click` 用 GPUI 窗口 API 执行；关闭走统一的
///   [`crate::request_close`]（托盘可用时隐藏/最小化而非退出）。
fn window_button(
    id: &'static str,
    name: IconName,
    close: bool,
    area: WindowControlArea,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover = if close { theme::accent() } else { theme::surface() };
    div()
        .id(id)
        .window_control_area(area)
        .w(px(46.0))
        .h(px(64.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_click(move |_ev, window, cx| on_click(window, cx))
        .hover(move |s| s.bg(hover))
        .child(icon(name, 16.0, theme::text_primary()))
}
