//! 歌手详情页：头部（头像/关注/播放热门）+ 热门歌曲 + 专辑 + 简介/相似歌手。

use std::sync::Arc;

use gpui_kit::base::{transition, StyledExt, Transition};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Artist, ArtistAlbum, ArtistDetail, Song};

use super::super::components::{detail_shell, offline_hint, sticky_bar, StickyBar};
use super::super::components::{song_row, IndexCell, SongRow};
use super::super::motion;
use super::super::playback::Playback;
use super::super::services::{task_failure, Services};
use super::super::smooth_scroll::{smooth_scroll, smooth_scroll_offset};
use super::super::{fmt_date, hint, load_error, PageEvent};
use super::dim;
use crate::theme::{self, spacing};

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
    /// 主接口全部失败（离线等）：展示失败态而非空数据（K8）。
    failed: bool,
    /// 是否已拿到响应（区分「加载中」与真空数据）。
    loaded: bool,
    /// 本次展示的是磁盘快照（在线请求失败时）。
    offline: bool,
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
            failed: false,
            loaded: false,
            offline: false,
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
        self.failed = false;
        let meta = self.services.meta.clone();
        handle.spawn(async move {
            let detail = siren_core::network::api::artist_detail(&client, id).await;
            let songs = siren_core::network::api::artist_top_songs(&client, id).await;
            let albums = siren_core::network::api::artist_albums(&client, id, 50).await;
            let similar = siren_core::network::api::similar_artists(&client, id).await;
            let follow = siren_core::network::api::artist_follow_count(&client, id).await;
            // 在线成功即写快照（离线时据此兜底）。
            if let (Some(meta), Ok(Some(d))) = (meta.as_ref(), &detail) {
                meta.save(&format!("artist-{id}"), d);
            }
            // 主接口全部失败视为整体失败（离线）；单项失败仍展示其余内容（K8）。
            let failed = detail.is_err() && songs.is_err() && albums.is_err();
            let _ = tx.send((
                detail.ok().flatten(),
                songs.unwrap_or_default(),
                albums.unwrap_or_default(),
                similar.unwrap_or_default(),
                follow.unwrap_or((0, false)),
                failed,
            ));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((detail, songs, albums, similar, (fans, followed), failed)) = payload {
                    // 在线失败：退回磁盘快照（离线可看），并标记为「离线缓存」。
                    let cached = if detail.is_none() {
                        this.services
                            .meta
                            .as_ref()
                            .and_then(|m| m.load::<ArtistDetail>(&format!("artist-{}", this.id)))
                    } else {
                        None
                    };
                    this.offline = detail.is_none() && cached.is_some();
                    let detail = detail.or(cached);
                    this.failed = failed && detail.is_none();
                    this.loaded = true;
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
            let _ = tx.send(siren_core::network::api::artist_follow(&client, id, follow).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let failure = task_failure(&result, "关注歌手");
            this.update(cx, |this, cx| {
                if let Some(msg) = failure {
                    this.followed = !follow;
                    this.fans += if follow { -1 } else { 1 };
                    this.services.toast_error(msg, cx);
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
        let weak_play = cx.weak_entity();
        let weak_follow = cx.weak_entity();
        let cover = self
            .detail
            .as_ref()
            .map(|d| d.cover_url.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();
        let avatar = self.services.covers.image_round(&cover, 108.0);

        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::LG))
            .child(avatar)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(spacing::XXS))
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
                            // 无热门歌曲时隐藏（点了无反应的假控件）。
                            .when(!self.top_songs.is_empty(), |el| {
                                el.child(Button::new("artist-play").primary().label("播放热门歌曲").on_click(
                                    move |_ev, _window, cx| {
                                        weak_play.update(cx, |this, cx| this.play_all(cx)).ok();
                                    },
                                ))
                            })
                            .child(
                                Button::new("artist-follow")
                                    .label(if self.followed { "已关注" } else { "关注" })
                                    .on_click(move |_ev, _window, cx| {
                                        weak_follow.update(cx, |this, cx| this.toggle_follow(cx)).ok();
                                    }),
                            ),
                    ),
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
            let weak = weak.clone();
            col = col.child(song_row(
                SongRow::new(
                    "artist-song",
                    song,
                    &self.playback,
                    &covers,
                    weak.clone(),
                    cx,
                    move |_ev, _window, cx| {
                        weak.update(cx, |this, cx| {
                            let items = this.top_songs.as_ref().clone();
                            this.playback.update(cx, |pb, cx| pb.play_from(items, i, cx));
                        })
                        .ok();
                    },
                )
                .row_h(dim::ROW_H_SM)
                .pad(spacing::XXS)
                // 序号列宽 24、字号 text_xs：与其余页的 `dim::INDEX_W` + text_sm 不同，原样保留。
                .index(IndexCell::number(i, 24.0, true))
                // 热门歌曲都出自该歌手，只显示歌名（不重复艺人行）。
                .show_artists(false),
            ));
        }
        col.into_any_element()
    }

    fn album_rows(&self, cx: &Context<Self>) -> AnyElement {
        let albums = self.albums.clone();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for album in albums.iter() {
            let cover = covers.image(&album.cover_url, dim::COVER_LIST);
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
                    .h(px(dim::ROW_H_LIST))
                    .px(px(spacing::XXS))
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_SM))
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
            let cover = covers.image_round(&artist.cover_url, dim::COVER_SONG);
            let weak = weak.clone();
            let (id, name) = (artist.id, artist.name.clone());
            col = col.child(
                div()
                    .id(format!("artist-simi-{id}"))
                    .w_full()
                    .h(px(dim::ROW_H))
                    .px(px(spacing::XXS))
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_SM))
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let mut body = div().v_flex().w_full().gap(px(18.0)).p(px(spacing::LG));
        if self.offline {
            body = body.child(offline_hint());
        }
        if self.failed {
            body = body.child(load_error(
                "artist-retry",
                cx.weak_entity(),
                "歌手信息加载失败",
                ArtistPage::load,
            ));
        }

        // 未返回时显示「加载中」，返回后真空才显示「暂无 XX」。
        body = body.child(section(
            "热门歌曲",
            if !self.loaded {
                hint("加载中…")
            } else if self.top_songs.is_empty() {
                hint("暂无歌曲")
            } else {
                self.song_rows(cx)
            },
        ));
        body = body.child(section(
            "专辑",
            if !self.loaded {
                hint("加载中…")
            } else if self.albums.is_empty() {
                hint("暂无专辑")
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

        let scroll = smooth_scroll("artist-scroll", body, window, cx);

        // 顶部 hero 随内容区滚动收起为紧凑横栏（对齐 Melodia `ArtistPage` 的同款 sticky bar：
        // 大图/名字滚走后，固定区只留「播放键 + 歌手名」，纵向空间还给内容）。
        let collapsed = smooth_scroll_offset("artist-scroll", cx) > dim::HEADER_COLLAPSE_AT_PX;
        let fold = transition(
            "artist-header-collapse",
            if collapsed { 1.0f32 } else { 0.0 },
            Transition::new(motion::HEADER_COLLAPSE).ease(motion::ease_in_out()),
            window,
            cx,
        );
        let weak_bar_play = cx.weak_entity();
        // 横栏与歌单/专辑页同形：[播放键][歌手名]。歌手页没有可取色的封面，故用面板底色。
        let mut bar = StickyBar::new(
            dim::STICKY_BAR_H * fold,
            self.name.clone(),
            "artist-bar-play",
            move |_ev, _window, cx| {
                weak_bar_play.update(cx, |this, cx| this.play_all(cx)).ok();
            },
        );
        bar.bg = Some(theme::pane());
        let sticky_bar = sticky_bar(bar);

        // hero 区占位高度 = `p(MD) * 2 + 头像 108`。
        const HERO_BLOCK_H: f32 = 132.0;
        detail_shell(
            div()
                .v_flex()
                .w_full()
                .child(
                    // padding 必须放内层：折叠容器自身带 padding 时，高度插值到 0 也仍占 padding，
                    // 收起后会在横栏上方留一条空白（hero 的 `p(MD)` 同理）。
                    div()
                        .w_full()
                        .h(px(HERO_BLOCK_H * (1.0 - fold)))
                        .overflow_hidden()
                        .opacity(1.0 - fold)
                        .child(div().w_full().p(px(spacing::MD)).child(header)),
                )
                .child(sticky_bar)
                .child(scroll)
                .into_any_element(),
        )
    }
}

fn section(title: &str, body: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(spacing::SM))
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

fn format_count(count: i64) -> String {
    if count >= 100_000_000 {
        format!("{:.1}亿", count as f64 / 100_000_000.0)
    } else if count >= 10_000 {
        format!("{:.1}万", count as f64 / 10_000.0)
    } else {
        count.to_string()
    }
}
