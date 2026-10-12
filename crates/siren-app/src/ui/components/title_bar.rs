//! 标题栏：前进/后退、首页、搜索框、浏览、下载、账户、窗口按钮。
//!
//! 对齐 Melodia 桌面端的 `TitleBar.kt`。

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::*;

use super::super::{icon, Root, Route};
use crate::theme;

impl Root {
    pub(crate) fn title_bar(&self, weak: WeakEntity<Self>, vw: f32, _cx: &App) -> impl IntoElement {
        let is_home = matches!(self.route(), Route::Home);

        let weak_back = weak.clone();
        let weak_home = weak.clone();
        let weak_search = weak;
        // 中间组宽度按视口显式分配，规避 gpui `flex_1` 的横向过度伸展
        let middle_w = (vw - 80.0 - 216.0 - 24.0).max(300.0);

        div()
            .w_full()
            .h(px(64.0))
            .flex()
            .items_center()
            .px(px(12.0))
            // 拖动区：按住标题栏空白处移动窗口；双击最大化/还原
            .on_mouse_down(MouseButton::Left, |ev, window, _cx| {
                if ev.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            // 左：前进 / 后退
            .child(
                div()
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
                    ),
            )
            // 中：首页 + 搜索框
            .child(
                div()
                    .w(px(middle_w))
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
                    ),
            )
            // 右：下载、账户、窗口按钮
            .child(
                div()
                    .flex()
                    .items_center()
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
                    .child(
                        div()
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme::surface())
                            .child(icon(IconName::CircleUser, 20.0, theme::text_gray())),
                    )
                    .child(window_button("win-min", IconName::Minus, false, |w, _cx| {
                        w.minimize_window();
                    }))
                    .child(window_button("win-max", IconName::Square, false, |w, _cx| {
                        w.zoom_window();
                    }))
                    .child(window_button("win-close", IconName::X, true, |w, _cx| {
                        w.remove_window();
                    })),
            )
    }
}

/// 窗口控制按钮：最小化 / 最大化 / 关闭。
fn window_button(id: &'static str, name: IconName, close: bool, action: fn(&mut Window, &mut App)) -> impl IntoElement {
    let hover = if close { theme::accent() } else { theme::surface() };
    div()
        .id(id)
        .w(px(46.0))
        .h(px(64.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(move |_ev, window, cx| action(window, cx))
        .child(icon(name, 16.0, theme::text_primary()))
}
