//! 首页：推荐歌单列表。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::Playlist;

use super::super::services::Services;
use super::super::{page_shell, PageEvent};
use crate::theme;

pub(crate) struct HomePage {
    services: Services,
    playlists: Arc<Vec<Playlist>>,
    status: SharedString,
}

impl EventEmitter<PageEvent> for HomePage {}

impl HomePage {
    pub(crate) fn new(cx: &mut Context<Self>, services: Services) -> Self {
        let mut page = Self {
            services,
            playlists: Arc::new(Vec::new()),
            status: "加载中…".into(),
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let list = siren_core::network::api::personalized_playlists(&client, 30)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        self.status = "加载推荐歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.status = format!("推荐歌单 {} 个", list.len()).into();
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.playlists = Arc::new(list);
                this.services.covers.prefetch(urls, 48.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playlists = self.playlists.clone();
        let count = playlists.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let list = uniform_list("home-playlists", count, move |range, _window, _cx| {
            range
                .map(|i| {
                    let pl = &playlists[i];
                    let cover = covers.image(&pl.cover_url, 48.0);
                    let weak = weak.clone();
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
                            weak.update(cx, |_page, cx| {
                                cx.emit(PageEvent::OpenPlaylist { id, name: name.clone() });
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

        page_shell(format!("首页 · {}", self.status), list.into_any_element())
    }
}
