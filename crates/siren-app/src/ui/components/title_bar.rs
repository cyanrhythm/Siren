//! 标题栏：前进/后退、首页、搜索框、下载、账户、窗口按钮。
//!
//! 对齐 Melodia 桌面端的 `TitleBar.kt`。
//!
//! 拖动区只放在**空白 spacer**上（`WindowControlArea::Drag`）；窗口按钮各自
//! `Min/Max/Close`。若把 Drag 设在整条容器上，父元素命中区会盖住按钮（命中测试
//! 返回第一个匹配的 area），导致最小化/最大化/关闭点击无反应。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::Icon;
use gpui_kit::*;

use super::super::{icon, PageEvent, Root, Route};
use crate::theme;

impl Root {
    pub(crate) fn title_bar(&self, weak: WeakEntity<Self>, vw: f32, cx: &App) -> impl IntoElement {
        let is_home = matches!(self.route(), Route::Home);

        let weak_back = weak.clone();
        let weak_home = weak.clone();
        let weak_account = weak.clone();
        let weak_search = weak;

        // 账号按钮：已登录显示圆形头像，未登录显示人形占位（对齐 Melodia）。
        // 用 Custom 变体避免 ghost 的 accent(红) hover/active 底，按钮与头像同尺寸的圆形。
        let covers = self.services.covers.clone();
        let account = self.account.clone();
        let trigger = match &account {
            Some((_, url, _)) => Button::new("account")
                .custom(ButtonCustomVariant::new(cx))
                .w(px(32.0))
                .h(px(32.0))
                .p(px(0.0))
                .rounded_full()
                .child(
                    div()
                        .size(px(32.0))
                        .rounded_full()
                        .overflow_hidden()
                        .child(self.services.covers.image(url, 32.0)),
                ),
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
                Some((nick, url, uid)) => {
                    let nick_render = nick.clone();
                    let url_render = url.clone();
                    let covers_render = covers.clone();
                    let uid = *uid;
                    menu = menu.item(PopupMenuItem::element(move |_w, _cx| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .py(px(4.0))
                            .child(covers_render.image(&url_render, 32.0))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .child(nick_render.clone()),
                            )
                    }));
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
            .gap(px(2.0))
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
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .child(
                div()
                    .id("nav-fwd")
                    .size(px(34.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .child(icon(IconName::ChevronRight, 18.0, theme::surface_light())),
            );

        // 中：首页 + 搜索框
        let middle = div()
            .w(px(472.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(
                div()
                    .id("home")
                    .size(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme::surface())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface_light()))
                    .on_click(move |_ev, _window, cx| {
                        weak_home.update(cx, |root, cx| root.navigate(Route::Home, cx)).ok();
                    })
                    .child(icon(
                        IconName::House,
                        20.0,
                        if is_home {
                            theme::text_primary()
                        } else {
                            theme::text_gray()
                        },
                    )),
            )
            .child(
                div()
                    .id("search-box")
                    .w(px(420.0))
                    .h(px(44.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .rounded(px(22.0))
                    .bg(theme::surface())
                    .on_click(move |_ev, _window, cx| {
                        weak_search.update(cx, |root, cx| root.navigate(Route::Search, cx)).ok();
                    })
                    .child(icon(IconName::Search, 18.0, theme::text_gray()))
                    .child(div().flex_1().child(Input::new(&self.search_input))),
            );

        // 右：下载、账户、窗口按钮
        let right = div()
            .w(px(224.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(4.0))
            .child(
                div()
                    .size(px(34.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .child(icon(IconName::Download, 18.0, theme::text_gray())),
            )
            .child(account_button)
            .child(window_button("win-min", IconName::Minus, false, WindowControlArea::Min))
            .child(window_button(
                "win-max",
                IconName::Square,
                false,
                WindowControlArea::Max,
            ))
            .child(window_button("win-close", IconName::X, true, WindowControlArea::Close));

        // 仅空白区可拖动（避免命中区盖住按钮）
        let fixed = 84.0 + 472.0 + 224.0;
        let sp = ((vw - 24.0 - fixed) / 2.0).max(0.0);
        let drag = |id: &'static str| {
            div()
                .id(id)
                .w(px(sp))
                .h(px(64.0))
                .window_control_area(WindowControlArea::Drag)
        };

        div()
            .w_full()
            .h(px(64.0))
            .flex()
            .items_center()
            .px(px(12.0))
            .child(left)
            .child(drag("tb-drag-l"))
            .child(middle)
            .child(drag("tb-drag-r"))
            .child(right)
    }
}

/// 窗口控制按钮：最小化 / 最大化 / 关闭（由 OS 处理）。
fn window_button(id: &'static str, name: IconName, close: bool, area: WindowControlArea) -> impl IntoElement {
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
        .hover(move |s| s.bg(hover))
        .child(icon(name, 16.0, theme::text_primary()))
}
