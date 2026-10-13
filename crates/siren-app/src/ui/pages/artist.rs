//! 歌手详情页：头部（头像/关注/播放热门）+ 热门歌曲 + 专辑 + 简介/相似歌手。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::*;

use siren_core::model::{Artist, ArtistAlbum, ArtistDetail, Song};

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{fmt_date, icon, page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct ArtistPage {
    services: Services,
    playback: Entity<Playback>,
    id: i64,
    name: String,
    detail: Option<ArtistDetail>,
    top_songs: Arc<Vec<Song>>,
    albums: Arc<Vec<ArtistAlbum>>,
    similar: Vec<Artist>,
    followed: bool,
    fans: i64,
    load_gen: u64,
}

impl EventEmitter<PageEvent> for ArtistPage {}

impl ArtistPage {
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
            top_songs: Arc::new(Vec::new()),
            albums: Arc::new(Vec::new()),
            similar: Vec::new(),
            followed: false,
            fans: 0,
            load_gen: 0,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let id = self.id;
        let handle = self.services.handle.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        handle.spawn(async move {
            let detail = siren_core::network::api::artist_detail(&client, id)
                .await
                .ok()
                .flatten();
            let songs = siren_core::network::api::artist_top_songs(&client, id)
                .await
                .unwrap_or_default();
            let albums = siren_core::network::api::artist_albums(&client, id, 50)
                .await
                .unwrap_or_default();
            let similar = siren_core::network::api::similar_artists(&client, id)
                .await
                .unwrap_or_default();
            let (fans, followed) = siren_core::network::api::artist_follow_count(&client, id)
                .await
                .unwrap_or((0, false));
            let _ = tx.send((detail, songs, albums, similar, fans, followed));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((detail, songs, albums, similar, fans, followed)) = payload {
                    let urls: Vec<String> = albums.iter().map(|a| a.cover_url.clone()).collect();
                    this.services.covers.prefetch(urls, 48.0, cx);
                    let song_covers: Vec<String> = songs.iter().map(|s| s.cover_url.clone()).collect();
                    this.services.covers.prefetch(song_covers, 40.0, cx);
                    let simi_covers: Vec<String> = similar.iter().map(|a| a.cover_url.clone()).collect();
                    this.services.covers.prefetch(simi_covers, 40.0, cx);
                    if let Some(d) = &detail {
                        this.services.covers.prefetch([d.cover_url.clone()], 108.0, cx);
                    }
                    this.detail = detail;
                    this.top_songs = Arc::new(songs);
                    this.albums = Arc::new(albums);
                    this.similar = similar;
                    this.fans = fans;
                    this.followed = followed;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 关注 / 取关；乐观更新，失败回滚并提示。
    fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        let follow = !self.followed;
        self.followed = follow;
        self.fans += if follow { 1 } else { -1 };
        cx.notify();
        let client = self.services.client.clone();
        let id = self.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::artist_follow(&client, id, follow)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if !ok {
                    this.followed = !follow;
                    this.fans += if follow { -1 } else { 1 };
                    this.services.toast_error("操作失败", cx);
                } else {
                    this.services
                        .toast(if follow { "已关注歌手" } else { "已取消关注" }, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn header(&self, cx: &Context<Self>) -> AnyElement {
        let weak_back = cx.weak_entity();
        let weak_play = cx.weak_entity();
        let weak_follow = cx.weak_entity();
        let cover = self
            .detail
            .as_ref()
            .map(|d| d.cover_url.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let avatar = self.services.covers.image_round(&cover, 108.0);
        let play_label = if self.top_songs.is_empty() {
            "播放热门"
        } else {
            "播放热门歌曲"
        };

        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(avatar)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(self.name.clone()),
                    )
                    .child(div().text_xs().text_color(theme::text_gray()).child(format!(
                        "{} 粉丝 · {} 张专辑 · {} 首歌",
                        format_count(self.fans),
                        self.detail.as_ref().map(|d| d.album_size).unwrap_or(0),
                        self.detail.as_ref().map(|d| d.music_size).unwrap_or(0),
                    )))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(Button::new("artist-play").primary().label(play_label).on_click(
                                move |_ev, _window, cx| {
                                    weak_play.update(cx, |this, cx| this.play_all(cx)).ok();
                                },
                            ))
                            .child(
                                Button::new("artist-follow")
                                    .label(if self.followed { "已关注" } else { "关注" })
                                    .on_click(move |_ev, _window, cx| {
                                        weak_follow.update(cx, |this, cx| this.toggle_follow(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("artist-back")
                    .size(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |_p, cx| cx.emit(PageEvent::Back)).ok();
                    })
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .into_any_element()
    }

    fn play_all(&mut self, cx: &mut Context<Self>) {
        let items = self.top_songs.as_ref().clone();
        if items.is_empty() {
            return;
        }
        self.playback.update(cx, |pb, cx| pb.play_from(items, 0, cx));
    }

    fn song_rows(&self, cx: &Context<Self>) -> AnyElement {
        let songs = self.top_songs.clone();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for (i, song) in songs.iter().enumerate() {
            let cover = covers.image(&song.cover_url, 40.0);
            let weak = weak.clone();
            let sid = song.id;
            col = col.child(
                div()
                    .id(format!("artist-song-{sid}"))
                    .w_full()
                    .h(px(48.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(6.0))
                    .hover(|s| s.bg(theme::card_surface()))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |this, cx| {
                            let items = this.top_songs.as_ref().clone();
                            this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                        })
                        .ok();
                    })
                    .child(
                        div()
                            .w(px(24.0))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(format!("{}", i + 1)),
                    )
                    .child(cover)
                    .child(
                        div().flex_1().min_w(px(0.0)).overflow_hidden().child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .truncate()
                                .child(song.name.clone()),
                        ),
                    )
                    .child(
                        div()
                            .w(px(56.0))
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(super::super::fmt_time(song.duration_ms)),
                    ),
            );
        }
        col.into_any_element()
    }

    fn album_rows(&self, cx: &Context<Self>) -> AnyElement {
        let albums = self.albums.clone();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for album in albums.iter() {
            let cover = covers.image(&album.cover_url, 48.0);
            let weak = weak.clone();
            let (id, name) = (album.id, album.name.clone());
            let meta = {
                let year = fmt_date(album.publish_time);
                let parts: Vec<String> = [
                    year,
                    if album.size > 0 {
                        format!("{} 首", album.size)
                    } else {
                        String::new()
                    },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                parts.join(" · ")
            };
            col = col.child(
                div()
                    .id(format!("artist-album-{id}"))
                    .w_full()
                    .h(px(56.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(6.0))
                    .hover(|s| s.bg(theme::card_surface()))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenAlbum { id, name: name.clone() });
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
                                    .child(album.name.clone()),
                            )
                            .child(div().text_xs().text_color(theme::text_gray()).child(meta)),
                    ),
            );
        }
        col.into_any_element()
    }

    fn similar_rows(&self, cx: &Context<Self>) -> AnyElement {
        let similar = self.similar.clone();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for artist in similar.iter() {
            let cover = covers.image_round(&artist.cover_url, 40.0);
            let weak = weak.clone();
            let (id, name) = (artist.id, artist.name.clone());
            col = col.child(
                div()
                    .id(format!("artist-simi-{id}"))
                    .w_full()
                    .h(px(52.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(6.0))
                    .hover(|s| s.bg(theme::card_surface()))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenArtist { id, name: name.clone() });
                        })
                        .ok();
                    })
                    .child(cover)
                    .child(
                        div().flex_1().min_w(px(0.0)).overflow_hidden().child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .truncate()
                                .child(artist.name.clone()),
                        ),
                    ),
            );
        }
        col.into_any_element()
    }
}

impl Render for ArtistPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let mut body = div().v_flex().flex_1().min_w(px(0.0)).gap(px(18.0)).p(px(16.0));

        body = body.child(section(
            "热门歌曲",
            if self.top_songs.is_empty() {
                empty_hint("暂无歌曲")
            } else {
                self.song_rows(cx)
            },
        ));
        body = body.child(section(
            "专辑",
            if self.albums.is_empty() {
                empty_hint("暂无专辑")
            } else {
                self.album_rows(cx)
            },
        ));

        let about: AnyElement = {
            let mut col = div().v_flex().w_full().gap(px(10.0));
            let brief = self
                .detail
                .as_ref()
                .map(|d| d.brief_desc.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "暂无艺人简介信息".to_string());
            col = col.child(div().text_xs().text_color(theme::text_gray()).child(brief));
            if !self.similar.is_empty() {
                col = col.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("相似歌手"),
                );
                col = col.child(self.similar_rows(cx));
            }
            col.into_any_element()
        };
        body = body.child(section("简介", about));

        let scroll = div()
            .id("artist-scroll")
            .flex_1()
            .min_w(px(0.0))
            .overflow_y_scroll()
            .child(body);
        page_shell_with_header(header, scroll.into_any_element())
    }
}

fn section(title: &str, body: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(8.0))
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(body)
        .into_any_element()
}

fn empty_hint(text: &str) -> AnyElement {
    div()
        .w_full()
        .py(px(12.0))
        .text_xs()
        .text_color(theme::text_gray())
        .child(text.to_string())
        .into_any_element()
}

fn format_count(count: i64) -> String {
    if count >= 100_000_000 {
        format!("{:.1}亿", count as f64 / 100_000_000.0)
    } else if count >= 10_000 {
        format!("{:.1}万", count as f64 / 10_000.0)
    } else {
        count.to_string()
    }
}
