//! 最近播放：顶部为「最近播放歌单 / 专辑」横向卡片，下方为按天分组的「最近播放歌曲」。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::{RecentAlbum, RecentPlaylist, RecentSong};

use super::super::components::like_button;
use super::super::playback::Playback;
use super::super::services::Services;
use super::super::session::Session;
use super::super::smooth_scroll::smooth_scroll;
use super::super::{fmt_day_label, load_error, page_shell_with_header, PageEvent};
use super::dim;
use crate::theme::{self, spacing};

pub(crate) struct RecentPage {
    services: Services,
    playback: Entity<Playback>,
    songs: Vec<RecentSong>,
    playlists: Vec<RecentPlaylist>,
    albums: Vec<RecentAlbum>,
    status: SharedString,
    load_gen: u64,
    /// 主接口全部失败（离线等）：展示失败态而非空数据（K8）。
    failed: bool,
}

impl EventEmitter<PageEvent> for RecentPage {}

impl RecentPage {
    pub(crate) fn new(services: Services, playback: Entity<Playback>) -> Self {
        Self {
            services,
            playback,
            songs: Vec::new(),
            playlists: Vec::new(),
            albums: Vec::new(),
            status: "最近播放".into(),
            load_gen: 0,
            failed: false,
        }
    }

    /// 拉取最近播放（登录后 / 进入页面时）。
    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.failed = false;
        self.services.handle.spawn(async move {
            let songs = siren_core::network::api::recent_songs_with_time(&client).await;
            let playlists = siren_core::network::api::recent_playlists(&client).await;
            let albums = siren_core::network::api::recent_albums(&client).await;
            // 全部失败视为整体失败（离线）；单项失败仍展示其余内容（K8）。
            let failed = songs.is_err() && playlists.is_err() && albums.is_err();
            let _ = tx.send((
                songs.unwrap_or_default(),
                playlists.unwrap_or_default(),
                albums.unwrap_or_default(),
                failed,
            ));
        });
        self.status = "加载最近播放…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((songs, playlists, albums, failed)) = payload {
                    this.failed = failed;
                    // 本地最近播放歌单与服务端合并去重（按播放时间倒序）
                    let mut merged: Vec<RecentPlaylist> = siren_core::recent_playlists::load_recent_playlists()
                        .into_iter()
                        .map(|p| RecentPlaylist {
                            id: p.id,
                            name: p.name,
                            cover_url: p.cover_url,
                            creator_name: String::new(),
                            play_time: p.play_time,
                        })
                        .collect();
                    merged.extend(playlists);
                    merged.sort_by_key(|p| std::cmp::Reverse(p.play_time));
                    let mut seen = std::collections::HashSet::new();
                    merged.retain(|p| seen.insert(p.id));
                    merged.truncate(100);

                    let covers: Vec<String> = songs.iter().map(|s| s.song.cover_url.clone()).collect();
                    this.services.covers.prefetch(covers, 40.0, cx);
                    this.services
                        .covers
                        .prefetch(merged.iter().map(|p| p.cover_url.clone()), 108.0, cx);
                    this.services
                        .covers
                        .prefetch(albums.iter().map(|a| a.cover_url.clone()), 108.0, cx);

                    this.status = format!("最近播放 · {} 首", songs.len()).into();
                    this.songs = songs;
                    this.playlists = merged;
                    this.albums = albums;
                } else {
                    this.status = "最近播放（需登录）".into();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn card_row(
        &self,
        id_prefix: &'static str,
        items: Vec<(i64, String, String)>,
        on_open: fn(i64, String) -> PageEvent,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        // 横向溢出处理：单行横向滚动（对齐首页货架卡片行），窄窗口不撑破布局。
        let mut row = div()
            .id(id_prefix)
            .flex()
            .gap(px(spacing::MD))
            .w_full()
            .overflow_x_scroll()
            .restrict_scroll_to_axis();
        for (id, name, cover_url) in items {
            let cover = covers.image(&cover_url, 108.0);
            let weak = weak.clone();
            let label = name.clone();
            row = row.child(
                div()
                    .id(format!("{id_prefix}-{id}"))
                    .w(px(108.0))
                    .flex_none()
                    .v_flex()
                    .gap(px(spacing::SM))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_page, cx| cx.emit(on_open(id, name.clone()))).ok();
                    })
                    .child(cover)
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(label),
                    ),
            );
        }
        row.into_any_element()
    }

    fn songs_grouped(&self, now_ms: i64, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let songs: Arc<Vec<RecentSong>> = Arc::new(self.songs.clone());
        let mut col = div().v_flex().w_full();
        let mut current_label = String::from("\u{0}");
        for (i, item) in songs.iter().enumerate() {
            let label = fmt_day_label(item.play_time, now_ms);
            if label != current_label {
                current_label = label.clone();
                if !label.is_empty() {
                    col = col.child(
                        div()
                            .w_full()
                            .pt(px(10.0))
                            .pb(px(spacing::XXS))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(label),
                    );
                }
            }
            let cover = covers.image(&item.song.cover_url, dim::COVER_SONG);
            let weak = weak.clone();
            let weak_ctx = weak.clone();
            let idx = i;
            let sid = item.song.id;
            let song_owned = item.song.clone();
            let liked = self.playback.read(cx).is_liked(sid);
            col = col.child(
                div()
                    .id(format!("recent-song-{sid}"))
                    .group(format!("recent-song-{sid}"))
                    .w_full()
                    .h(px(dim::ROW_H_SM))
                    .px(px(spacing::LG))
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_SM))
                    .hover(|s| s.bg(theme::card_surface()))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |this, cx| {
                            let items: Vec<siren_core::model::Song> =
                                this.songs.iter().map(|s| s.song.clone()).collect();
                            this.playback.update(cx, |pb, cx| pb.play_from(items, idx, cx));
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
                                    .child(item.song.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .truncate()
                                    .child(item.song.artists.clone()),
                            ),
                    )
                    .child(like_button(
                        &self.playback,
                        item.song.clone(),
                        format!("recent-song-{sid}"),
                        cx,
                    ))
                    .child(
                        div()
                            .w(px(dim::DURATION_W))
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(siren_core::recent_playlists::fmt_clock_time(item.play_time)),
                    )
                    .context_menu(move |menu, _w, _cx| {
                        let weak_next = weak_ctx.clone();
                        let weak_like = weak_ctx.clone();
                        let weak_add = weak_ctx.clone();
                        let song_next = song_owned.clone();
                        let song_like = song_owned.clone();
                        menu.item(PopupMenuItem::new("下一首播放").on_click(move |_ev, _w, cx| {
                            weak_next
                                .update(cx, |page, cx| {
                                    page.playback.update(cx, |pb, cx| pb.play_next(song_next.clone(), cx))
                                })
                                .ok();
                        }))
                        .item(PopupMenuItem::new(if liked { "取消喜欢" } else { "喜欢" }).on_click(
                            move |_ev, _w, cx| {
                                weak_like
                                    .update(cx, |page, cx| {
                                        page.playback
                                            .update(cx, |pb, cx| pb.toggle_like_song(song_like.clone(), cx))
                                    })
                                    .ok();
                            },
                        ))
                        .item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                            weak_add
                                .update(cx, |_page, cx| cx.emit(PageEvent::AddToPlaylist { song_id: sid }))
                                .ok();
                        }))
                    }),
            );
        }
        col.into_any_element()
    }
}

impl Render for RecentPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let header = div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(self.status.to_string())
            .into_any_element();

        let mut body = div().v_flex().w_full().p(px(spacing::LG)).gap(px(spacing::MD));
        let logged_in = Session::account(cx).is_some();
        if !logged_in {
            // 未登录：页面主体给空态 + 登录引导，而不是空壳。
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(spacing::SM))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child("登录后可查看最近播放"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child("点击标题栏账号按钮登录"),
                    ),
            );
        } else {
            if self.failed {
                body = body.child(load_error(
                    "recent-retry",
                    cx.weak_entity(),
                    "最近播放加载失败",
                    RecentPage::load,
                ));
            }

            let playlist_items: Vec<(i64, String, String)> = self
                .playlists
                .iter()
                .map(|p| (p.id, p.name.clone(), p.cover_url.clone()))
                .collect();
            if !playlist_items.is_empty() {
                body = body.child(section(
                    "最近播放歌单",
                    self.card_row(
                        "recent-pl",
                        playlist_items,
                        |id, name| PageEvent::OpenPlaylist { id, name },
                        cx,
                    ),
                ));
            }

            let album_items: Vec<(i64, String, String)> = self
                .albums
                .iter()
                .map(|a| (a.id, a.name.clone(), a.cover_url.clone()))
                .collect();
            if !album_items.is_empty() {
                body = body.child(section(
                    "最近播放专辑",
                    self.card_row(
                        "recent-al",
                        album_items,
                        |id, name| PageEvent::OpenAlbum { id, name },
                        cx,
                    ),
                ));
            }

            // 「最近播放歌曲」为空时不渲染该分组标题。
            if !self.songs.is_empty() {
                body = body.child(section("最近播放歌曲", self.songs_grouped(now_ms, cx)));
            }
        }

        page_shell_with_header(
            header,
            smooth_scroll("recent-scroll", body, window, cx).into_any_element(),
        )
    }
}

fn section(title: &str, content: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(spacing::SM))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(content)
        .into_any_element()
}
