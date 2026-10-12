//! 首页：推荐歌单列表。

use gpui_kit::base::StyledExt;
use gpui_kit::*;

use super::super::{page_shell, Root};
use crate::theme;

impl Root {
    pub(crate) fn home_page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let playlists = self.playlists.clone();
        let count = playlists.len();
        let covers = self.covers.clone();
        let weak_list = weak;
        let list = uniform_list("home-playlists", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let pl = &playlists[i];
                    let cover = covers.image(&pl.cover_url, 48.0);
                    let weak = weak_list.clone();
                    let id = pl.id;
                    let name = pl.name.clone();
                    div()
                        .id(format!("home-playlist-{id}"))
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

        page_shell(format!("首页 · {}", self.playlists_status), list.into_any_element())
    }
}
