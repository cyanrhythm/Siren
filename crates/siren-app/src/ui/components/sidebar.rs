//! 侧栏导航。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use super::super::{icon, Root, Route};
use crate::theme;

impl Root {
    pub(crate) fn sidebar(&self, weak: WeakEntity<Self>, cx: &App) -> impl IntoElement {
        let current = self.route();
        let nav_item =
            |id: &'static str, icon_name: IconName, label: &'static str, route: Route, weak: WeakEntity<Self>| {
                let active = matches!(
                    (&current, &route),
                    (Route::Home, Route::Home) | (Route::Library, Route::Library) | (Route::Search, Route::Search)
                );
                div()
                    .id(id)
                    .w_full()
                    .h(px(40.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .bg(if active { theme::surface() } else { theme::pane() })
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |this, cx| this.navigate(route.clone(), cx)).ok();
                    })
                    .child(icon(
                        icon_name,
                        18.0,
                        if active {
                            theme::text_primary()
                        } else {
                            theme::text_gray()
                        },
                    ))
                    .child(div().text_sm().text_color(theme::text_primary()).child(label))
            };

        let weak_home = weak.clone();
        let weak_library = weak.clone();
        let weak_search = weak.clone();
        div()
            .v_flex()
            .w(px(210.0))
            .min_w(px(210.0))
            .h_full()
            .flex_none()
            .flex_shrink_0()
            .bg(theme::pane())
            .p(px(12.0))
            .gap(px(4.0))
            .child(
                div()
                    .px(px(8.0))
                    .py(px(10.0))
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::accent())
                    .child("Siren"),
            )
            .child(nav_item("nav-home", IconName::House, "首页", Route::Home, weak_home))
            .child(nav_item(
                "nav-library",
                IconName::Heart,
                "音乐库",
                Route::Library,
                weak_library,
            ))
            .child(nav_item(
                "nav-search",
                IconName::Search,
                "搜索",
                Route::Search,
                weak_search,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .px(px(8.0))
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(self.playback.read(cx).status().clone()),
            )
    }
}
