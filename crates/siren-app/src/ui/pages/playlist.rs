//! 歌单详情：返回 + 曲目列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::PlaylistDetail;

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{fmt_time, icon, page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct PlaylistPage {
    services: Services,
    playback: Entity<Playback>,
    id: i64,
    name: String,
    detail: Option<Arc<PlaylistDetail>>,
    subscribed: bool,
}

impl EventEmitter<PageEvent> for PlaylistPage {}

impl PlaylistPage {
    pub(crate) fn new(
        cx: &mut Context<Self>,
        services: Services,
        playback: Entity<Playback>,
        id: i64,
        name: String,
    ) -> Self {
        let mut page = Self {
            services,
            playback,
            id,
            name,
            detail: None,
            subscribed: false,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let id = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let mut detail = siren_core::network::api::playlist_detail(&client, id)
                .await
                .ok()
                .flatten();
            // 曲目被截断时，按 trackIds 分批补全（对齐 Melodia PlaylistTrackPaging）
            if let Some(d) = &mut detail {
                if !d.track_ids.is_empty() && d.track_ids.len() > d.tracks.len() {
                    let mut all = Vec::new();
                    for chunk in d.track_ids.chunks(500) {
                        if let Ok(mut songs) = siren_core::network::api::song_details(&client, chunk).await {
                            all.append(&mut songs);
                        }
                    }
                    let mut map: std::collections::HashMap<i64, siren_core::model::Song> =
                        all.into_iter().map(|s| (s.id, s)).collect();
                    d.tracks = d.track_ids.iter().filter_map(|id| map.remove(id)).collect();
                }
            }
            let _ = tx.send(detail);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if let Some(d) = &detail {
                    this.subscribed = d.subscribed;
                }
                this.detail = detail.map(Arc::new);
                if let Some(d) = &this.detail {
                    let urls: Vec<String> = d.tracks.iter().map(|t| t.cover_url.clone()).collect();
                    this.services.covers.prefetch(urls, 40.0, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_subscribe(&mut self, cx: &mut Context<Self>) {
        let subscribe = !self.subscribed;
        self.subscribed = subscribe;
        cx.notify();
        let client = self.services.client.clone();
        let id = self.id;
        self.services.handle.spawn(async move {
            let _ = siren_core::network::api::playlist_subscribe(&client, id, subscribe).await;
        });
    }

    /// 从当前歌单移除某首歌，成功后重新加载曲目。
    fn remove_track(&mut self, song_id: i64, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let pid = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_delete_tracks(&client, pid, &[song_id])
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.load(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// 更换封面：选择图片 → NOS 直传 → 登记 → 重载。
    fn change_cover(&mut self, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择封面图片".into()),
        };
        let task = cx.prompt_for_paths(options);
        let client = self.services.client.clone();
        let handle = self.services.handle.clone();
        let pid = self.id;
        cx.spawn(async move |this, cx| {
            let picked = match task.await {
                Ok(Ok(paths)) => paths.and_then(|mut v| v.pop()),
                _ => None,
            };
            let Some(path) = picked else { return };
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_string();
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("cover.jpg")
                .to_string();
            let Ok(bytes) = std::fs::read(&path) else { return };
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle.spawn(async move {
                let ok = siren_core::network::api::update_playlist_cover(&client, pid, bytes, &ext, &filename)
                    .await
                    .is_ok();
                let _ = tx.send(ok);
            });
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.load(cx);
                    cx.emit(PageEvent::PlaylistChanged);
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for PlaylistPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_back = cx.weak_entity();
        let weak_star = cx.weak_entity();
        let weak_star2 = cx.weak_entity();
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .id("nav-back")
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |_page, cx| cx.emit(PageEvent::Back)).ok();
                    })
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .child(
                div()
                    .flex_1()
                    .text_lg()
                    .text_color(theme::text_primary())
                    .child(self.name.clone()),
            )
            .child(
                div()
                    .id("playlist-subscribe")
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click({
                        let weak_star = weak_star;
                        move |_ev, _window, cx| {
                            weak_star.update(cx, |p, cx| p.toggle_subscribe(cx)).ok();
                        }
                    })
                    .child(icon(
                        if self.subscribed {
                            IconName::Star
                        } else {
                            IconName::StarOff
                        },
                        18.0,
                        if self.subscribed {
                            theme::accent()
                        } else {
                            theme::text_gray()
                        },
                    )),
            )
            .child({
                let weak_menu = weak_star2;
                let pid = self.id;
                Button::new("pl-more")
                    .ghost()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu(move |menu, _w, _cx| {
                        let weak = weak_menu.clone();
                        let weak_cover = weak_menu.clone();
                        menu.item(PopupMenuItem::new("编辑简介").on_click(move |_ev, _w, cx| {
                            weak.update(cx, |_p, cx| cx.emit(PageEvent::EditPlaylistDesc { id: pid }))
                                .ok();
                        }))
                        .item(PopupMenuItem::new("更换封面").on_click(move |_ev, _w, cx| {
                            weak_cover.update(cx, |page, cx| page.change_cover(cx)).ok();
                        }))
                    })
            });

        let body: AnyElement = match &self.detail {
            Some(detail) => {
                let tracks = detail.tracks.clone();
                let count = tracks.len();
                let covers = self.services.covers.clone();
                let weak = cx.weak_entity();
                let rows = uniform_list("playlist-tracks", count, move |range, _window, _cx| {
                    range
                        .map(|i| {
                            let song = &tracks[i];
                            let cover = covers.image(&song.cover_url, 40.0);
                            let weak = weak.clone();
                            let weak_ctx = weak.clone();
                            let sid = song.id;
                            div()
                                .id(format!("playlist-track-{}", song.id))
                                .w_full()
                                .h(px(52.0))
                                .px(px(16.0))
                                .flex()
                                .items_center()
                                .gap(px(12.0))
                                .rounded(px(6.0))
                                .hover(|s| s.bg(theme::card_surface()))
                                .cursor_pointer()
                                .on_click(move |_ev, _window, cx| {
                                    weak.update(cx, |this, cx| {
                                        let items = this.detail.as_ref().map(|d| d.tracks.clone()).unwrap_or_default();
                                        this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                                    })
                                    .ok();
                                })
                                .child(
                                    div()
                                        .w(px(28.0))
                                        .text_sm()
                                        .text_color(theme::text_gray())
                                        .child(format!("{}", i + 1)),
                                )
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
                                                .child(song.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme::text_gray())
                                                .truncate()
                                                .child(song.artists.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(240.0))
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .truncate()
                                        .child(song.album.clone()),
                                )
                                .child(
                                    div()
                                        .w(px(56.0))
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .child(fmt_time(song.duration_ms)),
                                )
                                .context_menu(move |menu, _w, _cx| {
                                    let weak_add = weak_ctx.clone();
                                    let weak_del = weak_ctx.clone();
                                    menu.item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                                        weak_add
                                            .update(cx, |_page, cx| cx.emit(PageEvent::AddToPlaylist { song_id: sid }))
                                            .ok();
                                    }))
                                    .item(
                                        PopupMenuItem::new("从歌单移除").on_click(move |_ev, _w, cx| {
                                            weak_del.update(cx, |page, cx| page.remove_track(sid, cx)).ok();
                                        }),
                                    )
                                })
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                })
                .flex_1()
                .w_full()
                .min_w(px(0.0));

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .w_full()
                    .child(column_header())
                    .child(rows)
                    .into_any_element()
            }
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(theme::text_gray()).child("加载歌单中…"))
                .into_any_element(),
        };
        page_shell_with_header(header.into_any_element(), body)
    }
}

/// 曲目表头（与行内列宽对齐）。
fn column_header() -> impl IntoElement {
    div()
        .w_full()
        .h(px(32.0))
        .px(px(16.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b_1()
        .border_color(theme::surface())
        .text_xs()
        .text_color(theme::text_gray())
        .child(div().w(px(28.0)).child("#"))
        .child(div().w(px(40.0)))
        .child(div().flex_1().child("标题"))
        .child(div().w(px(240.0)).child("专辑"))
        .child(div().w(px(56.0)).child("时长"))
}
