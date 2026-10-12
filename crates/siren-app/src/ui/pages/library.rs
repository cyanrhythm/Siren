//! 音乐库：登录卡片 / 我的歌单。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::*;

use super::super::{page_shell, Root};
use crate::theme;

impl Root {
    pub(crate) fn library_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        if self.logged_in {
            let playlists = self.my_playlists.clone();
            let count = playlists.len();
            let covers = self.covers.clone();
            let weak_list = weak;
            let list = uniform_list("lib-playlists", count, move |range, _window, cx| {
                range
                    .map(|i| {
                        let pl = &playlists[i];
                        let cover = covers.image(&pl.cover_url, 48.0, &weak_list, cx);
                        let weak = weak_list.clone();
                        let id = pl.id;
                        let name = pl.name.clone();
                        div()
                            .id(format!("lib-playlist-{id}"))
                            .w_full()
                            .h(px(64.0))
                            .px(px(16.0))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .rounded(px(8.0))
                            .hover(|s| s.bg(theme::card_surface()))
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |this, cx| {
                                    this.open_playlist(id, name.clone(), cx);
                                })
                                .ok();
                            })
                            .child(cover)
                            .child(
                                div()
                                    .v_flex()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme::text_primary())
                                            .truncate()
                                            .child(pl.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_gray())
                                            .child(format!("{} 首", pl.track_count)),
                                    ),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
            .flex_1()
            .w_full()
            .min_w(px(0.0));

            page_shell(
                format!("音乐库 · {} · {}", self.nickname, self.login_status),
                list.into_any_element(),
            )
        } else {
            let weak_login = weak;
            let card = div()
                .v_flex()
                .gap(px(10.0))
                .w(px(440.0))
                .p(px(16.0))
                .rounded(px(10.0))
                .bg(theme::card_surface())
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("登录后查看你的歌单"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child("粘贴 MUSIC_U 或完整 Cookie（也可用 SIREN_COOKIE 环境变量）"),
                )
                .child(Input::new(&self.login_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("login")
                                .primary()
                                .label("登录")
                                .on_click(move |_ev, _window, cx| {
                                    weak_login.update(cx, |this, cx| this.do_login(cx)).ok();
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(self.login_status.clone()),
                        ),
                );
            page_shell(
                "音乐库".to_string(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(card)
                    .into_any_element(),
            )
        }
    }
}
