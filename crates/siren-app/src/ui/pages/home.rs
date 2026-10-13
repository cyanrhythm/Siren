//! 首页：顶部「全部 / 音乐 / 播客」胶囊；全部页为 最近播放 + 今日为你推荐 + 推荐歌单。
//!
//! 对齐 Melodia `desktopApp/ui/HomePage.kt`（各区块：`RecentGrid`、`DailySpotlight`、`ShelfRow`/`CardTile`）。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::{Playlist, RecentPlaylist, Song};

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
            let _ = tx.send((recent, daily, playlists));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((recent, daily, playlists)) = payload {
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
                    this.recent = merged;
                    this.daily = Arc::new(daily);
                    this.playlists = Arc::new(playlists);
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

    /// 最近播放：横向卡片网格（封面 + 标题），2 行 × N。
    fn recent_grid(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut grid = div().flex().flex_wrap().gap(px(12.0)).w_full();
        for item in &self.recent {
            let cover = covers.image(&item.cover_url, 72.0);
            let weak = weak.clone();
            let (id, name) = (item.id, item.name.clone());
            grid = grid.child(
                div()
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
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenPlaylist { id, name: name.clone() });
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
                    ),
            );
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

    /// 推荐歌单：封面卡片网格。
    fn playlist_grid(&self, cx: &Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        let covers = self.services.covers.clone();
        let mut grid = div().flex().flex_wrap().gap(px(16.0)).w_full();
        for pl in self.playlists.iter() {
            let cover = covers.image(&pl.cover_url, 168.0);
            let weak = weak.clone();
            let (id, name) = (pl.id, pl.name.clone());
            let count = pl.track_count;
            grid = grid.child(
                div()
                    .id(format!("home-pl-{id}"))
                    .w(px(168.0))
                    .v_flex()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenPlaylist { id, name: name.clone() });
                        })
                        .ok();
                    })
                    .child(cover)
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
                if !self.playlists.is_empty() {
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
