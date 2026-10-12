//! 搜索：搜索框 + 结果列表。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::Input;
use gpui_kit::*;

use super::super::{cover_placeholder, icon, page_shell, Root};
use crate::theme;

impl Root {
    pub(crate) fn search_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let songs = self.songs.clone();
        let count = songs.len();
        let selected = self.selected;
        let weak_list = weak;
        let list = uniform_list("search-results", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let song = &songs[i];
                    let marker = if selected == Some(i) { "▶ " } else { "   " };
                    let weak = weak_list.clone();
                    div()
                        .id(format!("search-song-{}", song.id))
                        .w_full()
                        .h(px(48.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(theme::card_surface()))
                        .cursor_pointer()
                        .on_click(move |_ev, _window, cx| {
                            weak.update(cx, |this, cx| {
                                let items = this.songs.as_ref().clone();
                                this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                            })
                            .ok();
                        })
                        .child(
                            div()
                                .w(px(24.0))
                                .text_sm()
                                .text_color(theme::text_gray())
                                .child(format!("{}", i + 1)),
                        )
                        .child(cover_placeholder(36.0))
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
                                        .child(format!("{marker}{}", song.name)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .truncate()
                                        .child(song.artists.clone()),
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
            format!("搜索 · {}", self.search_status),
            div()
                .v_flex()
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div().p(px(12.0)).child(
                        div()
                            .w(px(420.0))
                            .h(px(40.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .px(px(14.0))
                            .rounded(px(20.0))
                            .bg(theme::surface())
                            .child(icon(IconName::Search, 18.0, theme::text_gray()))
                            .child(div().flex_1().child(Input::new(&self.search_input))),
                    ),
                )
                .child(list)
                .into_any_element(),
        )
    }
}
