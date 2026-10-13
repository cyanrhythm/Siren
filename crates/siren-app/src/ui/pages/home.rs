//! 首页：顶部「全部 / 音乐 / 播客」胶囊；全部页为 最近播放 + 今日为你推荐 + 服务端货架。
//!
//! 对齐 Melodia `desktopApp/ui/HomePage.kt`（`RecentGrid`、`DailySpotlight`、`ShelfRow`/`CardTile`）。
//!
//! 卡片播放按钮：`group`/`group_hover` 做透明度显隐（纯样式、无重渲染、不闪烁）；
//! 点击用 `cx.stop_propagation()` 拦截封面/标题的“打开”行为（点播放=起播，点封面/标题=进详情）。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{HomeCard, HomeShelf, Playlist, RecentPlaylist, Song};

use super::super::playback::Playback;
use super::super::services::Services;
use super::super::{fmt_month_day, icon, PageEvent, PageLayout};
use crate::theme;

/// 首页卡片定宽与货架卡片间距（与 `card_tile` 保持一致，供货架宽度估算）。
const CARD_W: f32 = 168.0;
const CARD_GAP: f32 = 16.0;

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
    /// 每个货架的横向滚动句柄（跨帧保持滚动位置）。
    shelf_scroll: std::collections::HashMap<String, ScrollHandle>,
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
            shelf_scroll: std::collections::HashMap::new(),
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
                        .prefetch(merged.iter().map(|p| p.cover_url.clone()), 168.0, cx);
                    this.services
                        .covers
                        .prefetch(playlists.iter().map(|p| p.cover_url.clone()), 168.0, cx);
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
                    // 为每个货架建立跨帧保持的滚动句柄，否则每次 render 都会新建句柄，
                    // 翻页箭头 set_offset 后一重渲染即被丢弃，表现为「点了没反应」。
                    this.shelf_scroll
                        .retain(|code, _| shelves.iter().any(|s| &s.block_code == code));
                    for shelf in &shelves {
                        this.shelf_scroll
                            .entry(shelf.block_code.clone())
                            .or_insert_with(ScrollHandle::new);
                    }
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

    /// 最近播放：方形卡片（与货架同款）。
    fn recent_grid(&self, cx: &Context<Self>) -> AnyElement {
        let mut grid = div().flex().flex_wrap().gap(px(16.0)).w_full();
        for item in &self.recent {
            let card = HomeCard::Playlist {
                id: item.id,
                title: item.name.clone(),
                cover_url: item.cover_url.clone(),
                caption: String::new(),
            };
            grid = grid.child(self.card_tile(&card, None, cx));
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

    /// 推荐歌单（服务端货架缺失时的兜底）：方形卡片网格。
    fn playlist_grid(&self, cx: &Context<Self>) -> AnyElement {
        let mut grid = div().grid().grid_cols(6).gap(px(16.0)).w_full();
        for pl in self.playlists.iter() {
            let card = HomeCard::Playlist {
                id: pl.id,
                title: pl.name.clone(),
                cover_url: pl.cover_url.clone(),
                caption: String::new(),
            };
            grid = grid.child(self.card_tile(&card, None, cx));
        }
        grid.into_any_element()
    }

    /// 服务端货架：标题（悬停显示翻页箭头）+ 横向滚动卡片行。
    ///
    /// 箭头仅在内容宽度超过可视宽度（需要滚动）时才渲染；且悬停货架时才显形。
    fn shelf_section(&self, shelf: &HomeShelf, cx: &Context<Self>) -> AnyElement {
        let code = shelf.block_code.clone();
        let handle = self.shelf_scroll.get(&code).cloned().unwrap_or_default();
        let off = handle.offset();
        let max = handle.max_offset();

        // 内容实际宽度（卡片定宽 + 间距），用于首帧判断是否需要滚动。
        let n = shelf.cards.len() as f32;
        let content_w = if n > 0.0 {
            n * CARD_W + (n - 1.0) * CARD_GAP
        } else {
            0.0
        };
        let avail_w = (cx.global::<PageLayout>().page_w - 48.0).max(0.0);
        // 已测量则用真实可滚动距离，未测量（首帧）则用估算值。
        let measured = f32::from(max.x);
        let scroll_w = if measured > 0.5 {
            measured
        } else {
            (content_w - avail_w).max(0.0)
        };
        let can_left = scroll_w > 0.5 && f32::from(off.x) < -0.5;
        let can_right = scroll_w > 0.5 && f32::from(off.x) > -scroll_w + 0.5;
        let group = format!("shelf-group-{code}");

        let mut cards = div()
            .id(format!("shelf-row-{code}"))
            .flex()
            .gap(px(CARD_GAP))
            .w_full()
            .overflow_x_scroll()
            .restrict_scroll_to_axis()
            .track_scroll(&handle);
        for (i, card) in shelf.cards.iter().enumerate() {
            cards = cards.child(
                div()
                    .flex_none()
                    .child(self.card_tile(card, shelf.show_rank.then_some(i + 1), cx)),
            );
        }

        let mut title_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .w_full()
            .child(div().flex_1().min_w(px(0.0)).child(section_title(&shelf.title)));
        if can_left {
            title_row = title_row.child(scroll_arrow(
                format!("{code}-prev"),
                group.clone(),
                handle.clone(),
                true,
                cx.weak_entity(),
            ));
        }
        if can_right {
            title_row = title_row.child(scroll_arrow(
                format!("{code}-next"),
                group.clone(),
                handle.clone(),
                false,
                cx.weak_entity(),
            ));
        }

        let weak = cx.weak_entity();
        div()
            .id(format!("shelf-{code}"))
            .group(group)
            .v_flex()
            .w_full()
            .gap(px(10.0))
            .on_hover(move |_hv, _w, cx| {
                // 悬停时刷新一次：让首帧尚未测量到的 max_offset 生效，箭头显隐随之修正。
                weak.update(cx, |_page, cx| cx.notify()).ok();
            })
            .child(title_row)
            .child(div().w_full().min_w(px(0.0)).child(cards))
            .into_any_element()
    }

    /// 货架/网格卡片：封面（点击进详情）+ 类型角标 + 标题 + 描述；播放按钮悬停显示。
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
        let click_card = card.clone();
        let play_card = card.clone();
        let weak_open = weak.clone();
        let weak_play = weak.clone();
        let group = format!("home-card-{key}");
        let play_group = group.clone();
        let cover_click = div()
            .id(format!("home-card-open-{key}"))
            .size_full()
            .cursor_pointer()
            .on_click(move |_ev, _window, cx| {
                weak_open.update(cx, |_p, cx| open_card_event(&click_card, cx)).ok();
            })
            .child(cover);
        let media = div()
            .relative()
            .w(px(CARD_W))
            .h(px(CARD_W))
            .group(group.clone())
            .child(cover_click)
            .child(
                div()
                    .id(format!("home-card-play-{key}"))
                    .absolute()
                    .right(px(2.0))
                    .bottom(px(2.0))
                    .size(px(52.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .opacity(0.0)
                    .group_hover(play_group, |s| s.opacity(1.0))
                    .on_click(move |_ev, _window, cx| {
                        // 冒泡阶段按绘制顺序逆序派发，本按钮在封面之上，先于封面收到点击，
                        // 因此 stop_propagation 可拦截封面的“打开”行为，无需 occlude。
                        cx.stop_propagation();
                        weak_play.update(cx, |page, cx| page.play_card(&play_card, cx)).ok();
                    })
                    .child(play_fab(40.0)),
            );
        let text_card = card.clone();
        let weak_text = weak.clone();
        div()
            .id(format!("home-card-{key}"))
            .w(px(CARD_W))
            .v_flex()
            .gap(px(6.0))
            .child(media)
            .child(
                div()
                    .id(format!("home-card-text-{key}"))
                    .v_flex()
                    .w_full()
                    .gap(px(2.0))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak_text.update(cx, |_p, cx| open_card_event(&text_card, cx)).ok();
                    })
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
                    }),
            )
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

/// 卡片点击：歌单/专辑进入详情；单曲/播客不处理（走播放按钮）。
fn open_card_event(card: &HomeCard, cx: &mut Context<HomePage>) {
    match card {
        HomeCard::Playlist { id, title, .. } => {
            cx.emit(PageEvent::OpenPlaylist {
                id: *id,
                name: title.clone(),
            });
        }
        HomeCard::Album { id, title, .. } => {
            cx.emit(PageEvent::OpenAlbum {
                id: *id,
                name: title.clone(),
            });
        }
        HomeCard::Song { .. } | HomeCard::Voice { .. } => {}
    }
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

/// 货架翻页箭头（浅色圆形，悬停货架时显形）。
fn scroll_arrow(id: String, group: String, handle: ScrollHandle, back: bool, weak: WeakEntity<HomePage>) -> AnyElement {
    div()
        .id(id)
        .size(px(34.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::surface_light())
        .cursor_pointer()
        .opacity(0.0)
        .group_hover(group, |s| s.opacity(1.0))
        .hover(|s| s.bg(theme::surface()))
        .on_click(move |_ev, _window, cx| {
            let off = handle.offset();
            let max = handle.max_offset();
            // gpui 的滚动偏移为负值，合法范围 [-(内容-视口), 0]
            let step = 800.0f32;
            let nx = if back {
                (f32::from(off.x) + step).min(0.0)
            } else {
                (f32::from(off.x) - step).max(-f32::from(max.x))
            };
            handle.set_offset(point(px(nx), off.y));
            weak.update(cx, |_page, cx| cx.notify()).ok();
        })
        .child(icon(
            if back {
                IconName::ChevronLeft
            } else {
                IconName::ChevronRight
            },
            18.0,
            theme::text_primary(),
        ))
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
