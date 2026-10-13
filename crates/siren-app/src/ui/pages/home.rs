//! 首页：顶部「全部 / 音乐 / 播客」胶囊；全部页为 最近播放 + 今日为你推荐 + 推荐歌单。
//!
//! 对齐 Melodia `desktopApp/ui/HomePage.kt`（各区块：`RecentGrid`、`DailySpotlight`、`ShelfRow`/`CardTile`）。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{HomeCard, HomeShelf, Playlist, RecentPlaylist, Song};

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{fmt_month_day, icon, PageEvent};
use crate::theme;

/// 首页顶部胶囊。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HomeTab {
    All,
    Music,
    Podcast,
}

pub(crate) struct HomePage {
    services: Services,
    playback: Entity<Playback>,
    tab: HomeTab,
    recent: Vec<RecentPlaylist>,
    daily: Arc<Vec<Song>>,
    playlists: Arc<Vec<Playlist>>,
    shelves: Vec<HomeShelf>,
    /// 当前悬停的条目 id（用于显示播放按钮）。
    hovered: Option<i64>,
    load_gen: u64,
}

impl EventEmitter<PageEvent> for HomePage {}

impl HomePage {
    pub(crate) fn new(cx: &mut Context<Self>, services: Services, playback: Entity<Playback>) -> Self {
        let mut page = Self {
            services,
            playback,
            tab: HomeTab::All,
            recent: Vec::new(),
            daily: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            shelves: Vec::new(),
            hovered: None,
            load_gen: 0,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let recent = siren_core::network::api::recent_playlists(&client)
                .await
                .unwrap_or_default();
            let daily = siren_core::network::api::daily_recommend_songs(&client)
                .await
                .unwrap_or_default();
            let playlists = siren_core::network::api::personalized_playlists(&client, 30)
                .await
                .unwrap_or_default();
            let shelves = siren_core::network::api::home_shelves(&client)
                .await
                .unwrap_or_default();
            let _ = tx.send((recent, daily, playlists, shelves));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((recent, daily, playlists, shelves)) = payload {
                    // 本地最近播放歌单与服务端合并去重
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
                    merged.extend(recent);
                    merged.sort_by_key(|p| std::cmp::Reverse(p.play_time));
                    let mut seen = std::collections::HashSet::new();
                    merged.retain(|p| seen.insert(p.id));
                    merged.truncate(8);

                    this.services
                        .covers
                        .prefetch(merged.iter().map(|p| p.cover_url.clone()), 128.0, cx);
                    this.services
                        .covers
                        .prefetch(playlists.iter().map(|p| p.cover_url.clone()), 256.0, cx);
                    if let Some(first) = daily.first() {
                        this.services.covers.prefetch([first.cover_url.clone()], 128.0, cx);
                    }
                    for shelf in &shelves {
                        let covers: Vec<String> = shelf.cards.iter().map(|c| c.cover_url().to_string()).collect();
                        this.services.covers.prefetch(covers, 168.0, cx);
                    }
                    this.recent = merged;
                    this.daily = Arc::new(daily);
                    this.playlists = Arc::new(playlists);
                    this.shelves = shelves;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_tab(&mut self, tab: HomeTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn pills(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let mut row = div().flex().items_center().gap(px(8.0)).px(px(24.0)).pt(px(16.0));
        for (tab, label) in [
            (HomeTab::All, "全部"),
            (HomeTab::Music, "音乐"),
            (HomeTab::Podcast, "播客"),
        ] {
            let active = self.tab == tab;
            let weak = weak.clone();
            row = row.child(
                div()
                    .id(format!("home-tab-{label}"))
                    .px(px(16.0))
                    .py(px(6.0))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if active {
                        theme::text_primary()
                    } else {
                        theme::surface()
                    })
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |page, cx| page.select_tab(tab, cx)).ok();
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(if active {
                                theme::window_bg()
                            } else {
                                theme::text_primary()
                            })
                            .child(label),
                    ),
            );
        }
        row.into_any_element()
    }

    /// 最近播放：横向卡片网格（封面 + 标题），悬停显示红色播放按钮。
    fn recent_grid(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut grid = div().flex().flex_wrap().gap(px(12.0)).w_full();
        for item in &self.recent {
            let cover = covers.image(&item.cover_url, 72.0);
            let weak_click = weak.clone();
            let weak_hover = weak.clone();
            let weak_play = weak.clone();
            let (id, cover_url) = (item.id, item.cover_url.clone());
            let name_click = item.name.clone();
            let name_play = item.name.clone();
            let mut tile = div()
                .id(format!("home-recent-{id}"))
                .w(px(300.0))
                .h(px(72.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme::surface())
                .overflow_hidden()
                .cursor_pointer()
                .hover(|s| s.bg(theme::surface_light()))
                .on_hover(move |hovered, _w, cx| {
                    weak_hover
                        .update(cx, |page, cx| {
                            if *hovered {
                                page.hovered = Some(id);
                            } else if page.hovered == Some(id) {
                                page.hovered = None;
                            }
                            cx.notify();
                        })
                        .ok();
                })
                .on_click(move |_ev, _window, cx| {
                    weak_click
                        .update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenPlaylist {
                                id,
                                name: name_click.clone(),
                            })
                        })
                        .ok();
                })
                .child(cover)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .px(px(12.0))
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .truncate()
                        .child(item.name.clone()),
                );
            if self.hovered == Some(id) {
                tile = tile.child(
                    div()
                        .id(format!("home-recent-play-{id}"))
                        .pr(px(12.0))
                        .cursor_pointer()
                        .on_click(move |_ev, _window, cx| {
                            weak_play
                                .update(cx, |page, cx| {
                                    page.playback.update(cx, |pb, cx| {
                                        pb.play_playlist(id, name_play.clone(), cover_url.clone(), cx)
                                    })
                                })
                                .ok();
                        })
                        .child(play_fab(36.0)),
                );
            }
            grid = grid.child(tile);
        }
        grid.into_any_element()
    }

    /// 今日为你推荐：每日推荐聚焦卡（封面 + 标题 + 日期 + 曲目预览 + 播放）。
    fn daily_spotlight(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let first = self.daily.first()?;
        let weak = cx.weak_entity();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cover = self.services.covers.image(&first.cover_url, 72.0);
        let base = self.services.covers.color(&first.cover_url);
        let bg = base.map(|c| darken(c, 0.45)).unwrap_or_else(theme::card_surface);
        let preview: String = self
            .daily
            .iter()
            .take(5)
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        let count = self.daily.len();
        Some(
            div()
                .id("home-daily")
                .w_full()
                .flex()
                .items_center()
                .gap(px(16.0))
                .p(px(16.0))
                .rounded(px(8.0))
                .bg(bg)
                .cursor_pointer()
                .on_click(move |_ev, _window, cx| {
                    weak.update(cx, |page, cx| page.play_daily(cx)).ok();
                })
                .child(cover)
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_xl()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::text_primary())
                                .child("每日推荐"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(format!("根据你的口味 · {}", fmt_month_day(now_ms))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .truncate()
                                .child(format!("{count} 首 · {preview}")),
                        ),
                )
                .child(play_circle())
                .into_any_element(),
        )
    }

    fn play_daily(&mut self, cx: &mut Context<Self>) {
        let songs = self.daily.as_ref().clone();
        if songs.is_empty() {
            return;
        }
        self.playback.update(cx, |pb, cx| pb.play_from(songs, 0, cx));
    }

    /// 服务端货架：标题 + 卡片网格（类型角标 + 标题 + 描述）。
    fn shelf_section(&self, shelf: &HomeShelf, cx: &Context<Self>) -> AnyElement {
        let mut grid = div().flex().flex_wrap().gap(px(16.0)).w_full();
        for (index, card) in shelf.cards.iter().enumerate() {
            grid = grid.child(self.card_tile(card, shelf.show_rank.then_some(index + 1), cx));
        }
        div()
            .v_flex()
            .w_full()
            .gap(px(10.0))
            .child(section_title(&shelf.title))
            .child(grid)
            .into_any_element()
    }

    fn card_tile(&self, card: &HomeCard, rank: Option<usize>, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let key = match card {
            HomeCard::Voice { program_id, .. } => *program_id,
            HomeCard::Playlist { id, .. } | HomeCard::Album { id, .. } | HomeCard::Song { id, .. } => *id,
        };
        let cover = self.services.covers.image(card.cover_url(), 168.0);
        let title = card.title().to_string();
        let caption = card.caption().to_string();
        let type_label = match rank {
            Some(r) => format!("{} · {}", card.type_label(), r),
            None => card.type_label().to_string(),
        };
        let card_owned = card.clone();
        let weak_click = weak.clone();
        let weak_hover = weak.clone();
        let weak_play = weak.clone();
        let cover_box = div()
            .relative()
            .w(px(168.0))
            .h(px(168.0))
            .child(cover)
            .when(self.hovered == Some(key), |el| {
                el.child(
                    div()
                        .id(format!("home-card-play-{key}"))
                        .absolute()
                        .right(px(8.0))
                        .bottom(px(8.0))
                        .cursor_pointer()
                        .on_click(move |_ev, _window, cx| {
                            weak_play.update(cx, |page, cx| page.play_card(&card_owned, cx)).ok();
                        })
                        .child(play_fab(40.0)),
                )
            });
        let click_card = card.clone();
        div()
            .id(format!("home-card-{key}"))
            .w(px(168.0))
            .v_flex()
            .gap(px(6.0))
            .cursor_pointer()
            .on_hover(move |hovered, _w, cx| {
                weak_hover
                    .update(cx, |page, cx| {
                        if *hovered {
                            page.hovered = Some(key);
                        } else if page.hovered == Some(key) {
                            page.hovered = None;
                        }
                        cx.notify();
                    })
                    .ok();
            })
            .on_click(move |_ev, _window, cx| {
                weak_click
                    .update(cx, |_p, cx| match &click_card {
                        HomeCard::Playlist { id, title, .. } => cx.emit(PageEvent::OpenPlaylist {
                            id: *id,
                            name: title.clone(),
                        }),
                        HomeCard::Album { id, title, .. } => cx.emit(PageEvent::OpenAlbum {
                            id: *id,
                            name: title.clone(),
                        }),
                        HomeCard::Song { .. } | HomeCard::Voice { .. } => {}
                    })
                    .ok();
            })
            .child(cover_box)
            .child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(type_label),
            )
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .truncate()
                    .child(title),
            )
            .when(!caption.is_empty(), |el| {
                el.child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .truncate()
                        .child(caption),
                )
            })
            .into_any_element()
    }

    fn play_card(&mut self, card: &HomeCard, cx: &mut Context<Self>) {
        match card {
            HomeCard::Playlist {
                id, title, cover_url, ..
            } => {
                self.playback
                    .update(cx, |pb, cx| pb.play_playlist(*id, title.clone(), cover_url.clone(), cx));
            }
            HomeCard::Album {
                id, title, cover_url, ..
            } => {
                self.playback
                    .update(cx, |pb, cx| pb.play_album(*id, title.clone(), cover_url.clone(), cx));
            }
            HomeCard::Song { id, .. } => {
                self.playback.update(cx, |pb, cx| pb.play_song(*id, cx));
            }
            HomeCard::Voice { song_id, .. } => {
                self.playback.update(cx, |pb, cx| pb.play_song(*song_id, cx));
            }
        }
    }

    /// 推荐歌单：封面卡片网格，悬停时封面右下角显示红色播放按钮。
    fn playlist_grid(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut grid = div().flex().flex_wrap().gap(px(16.0)).w_full();
        for pl in self.playlists.iter() {
            let cover = covers.image(&pl.cover_url, 168.0);
            let weak_click = weak.clone();
            let weak_hover = weak.clone();
            let weak_play = weak.clone();
            let (id, cover_url, count) = (pl.id, pl.cover_url.clone(), pl.track_count);
            let name_click = pl.name.clone();
            let name_play = pl.name.clone();
            let cover_box =
                div()
                    .relative()
                    .w(px(168.0))
                    .h(px(168.0))
                    .child(cover)
                    .when(self.hovered == Some(id), |el| {
                        el.child(
                            div()
                                .id(format!("home-pl-play-{id}"))
                                .absolute()
                                .right(px(8.0))
                                .bottom(px(8.0))
                                .cursor_pointer()
                                .on_click(move |_ev, _window, cx| {
                                    weak_play
                                        .update(cx, |page, cx| {
                                            page.playback.update(cx, |pb, cx| {
                                                pb.play_playlist(id, name_play.clone(), cover_url.clone(), cx)
                                            })
                                        })
                                        .ok();
                                })
                                .child(play_fab(40.0)),
                        )
                    });
            grid = grid.child(
                div()
                    .id(format!("home-pl-{id}"))
                    .w(px(168.0))
                    .v_flex()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_hover(move |hovered, _w, cx| {
                        weak_hover
                            .update(cx, |page, cx| {
                                if *hovered {
                                    page.hovered = Some(id);
                                } else if page.hovered == Some(id) {
                                    page.hovered = None;
                                }
                                cx.notify();
                            })
                            .ok();
                    })
                    .on_click(move |_ev, _window, cx| {
                        weak_click
                            .update(cx, |_p, cx| {
                                cx.emit(PageEvent::OpenPlaylist {
                                    id,
                                    name: name_click.clone(),
                                })
                            })
                            .ok();
                    })
                    .child(cover_box)
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(pl.name.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(format!("{} 首", count)),
                    ),
            );
        }
        grid.into_any_element()
    }
}

impl Render for HomePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match self.tab {
            HomeTab::All => {
                let mut col = div()
                    .id("home-scroll")
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(28.0))
                    .p(px(24.0))
                    .overflow_y_scroll();
                if !self.recent.is_empty() {
                    col = col.child(section_title("最近播放")).child(self.recent_grid(cx));
                }
                if let Some(daily) = self.daily_spotlight(cx) {
                    col = col.child(section_title("今日为你推荐")).child(daily);
                }
                if !self.shelves.is_empty() {
                    for shelf in &self.shelves {
                        col = col.child(self.shelf_section(shelf, cx));
                    }
                } else if !self.playlists.is_empty() {
                    col = col.child(section_title("推荐歌单")).child(self.playlist_grid(cx));
                }
                col.into_any_element()
            }
            HomeTab::Music | HomeTab::Podcast => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(theme::text_gray()).child("该板块开发中"))
                .into_any_element(),
        };

        div()
            .v_flex()
            .size_full()
            .bg(theme::pane())
            .child(self.pills(cx))
            .child(body)
    }
}

fn section_title(text: &str) -> AnyElement {
    div()
        .text_2xl()
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text_primary())
        .child(text.to_string())
        .into_any_element()
}

fn play_circle() -> AnyElement {
    div()
        .size(px(48.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::text_primary())
        .child(icon(IconName::Play, 24.0, theme::window_bg()))
        .into_any_element()
}

/// 红色圆形播放按钮（品牌强调色底 + 播放三角）。
fn play_fab(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::accent())
        .child(icon(IconName::Play, size * 0.55, theme::text_primary()))
}

/// 压暗颜色（`factor` 越大越暗）。
fn darken(color: Rgba, factor: f32) -> Rgba {
    let f = (1.0 - factor).clamp(0.0, 1.0);
    Rgba {
        r: color.r * f,
        g: color.g * f,
        b: color.b * f,
        a: color.a,
    }
}
