//! 正在播放栏（右侧）：可上下滚动的卡片堆叠。
//!
//! 对齐 Melodia `desktopApp/ui/NowPlayingPanel.kt`：封面 + 标题/歌手 + 信息卡
//! （歌词预览 / 评论 / 歌曲详情 / 回忆坐标），整体竖向滚动。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Song, SongWiki};

use super::super::settings_store::SettingsStore;
use super::super::smooth_scroll::{smooth_scroll, smooth_scroll_plain};
use super::super::{cover_placeholder, fmt_date, icon, NpCard, Root};
use crate::theme::{self, spacing};
use siren_core::player::{PlaySource, PlaySourceKind};

impl Root {
    pub(crate) fn now_playing(
        &self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.np.open {
            let weak_expand = weak;
            return div()
                .id("shell-now-playing-rail")
                .v_flex()
                .w(px(44.0))
                .min_w(px(44.0))
                .h_full()
                .flex_none()
                .flex_shrink_0()
                .rounded(px(theme::RADIUS_MD))
                .bg(theme::pane())
                .items_center()
                .justify_center()
                .child(toggle_button("np-expand", IconName::ChevronLeft, weak_expand))
                .into_any_element();
        }

        let state = self.playback.read(cx).state();
        let source = self.playback.read(cx).source().cloned();
        let context = self.playback.read(cx).context().map(str::to_string);
        let song = state.song.clone();
        let (title, artist) = match &song {
            Some(s) => (s.name.clone(), s.artists.clone()),
            None => ("未在播放".to_string(), String::new()),
        };
        // 封面随面板「有效宽」放大（封顶 420，对齐 Melodia NowPlayingMaxWidth）：
        // 窗口缩窄时右栏显示宽由 np_effective_w 联动，封面不再按原始拖放宽被裁。
        let pads = gpui_kit::component::window_paddings(window);
        let vw = f32::from(window.viewport_size().width) - f32::from(pads.left) - f32::from(pads.right);
        let np_width = self.np_effective_w(vw, cx);
        let cover_size = (np_width - 28.0).clamp(220.0, 420.0);
        let cover: AnyElement = match &song {
            Some(s) => self.services.covers.image(&s.cover_url, cover_size),
            None => cover_placeholder(200.0).into_any_element(),
        };
        let detail = self.playback.read(cx).current_detail().cloned();
        let lyrics = self.playback.read(cx).current_lyrics().to_vec();
        let wiki = self.playback.read(cx).current_wiki().cloned();
        // 封面主色作为面板背景（对齐 Melodia CoverBackdrop）。
        let base_color = song.as_ref().and_then(|s| self.services.covers.color(&s.cover_url));
        let panel_bg = base_color.map(|c| darken(c, 0.28)).unwrap_or_else(theme::pane);

        // 卡片流（竖向滚动，平滑）。
        let mut body = div()
            .v_flex()
            .w_full()
            .gap(px(14.0))
            .child(div().w_full().flex().justify_center().child(cover))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(spacing::XXS))
                    .child(
                        div()
                            .text_color(theme::text_primary())
                            .font_weight(FontWeight::BOLD)
                            .child(title),
                    )
                    .child(div().text_sm().text_color(theme::text_gray()).truncate().child(artist)),
            );

        let comments = self.playback.read(cx).comments().clone();
        let artist = self.playback.read(cx).artist().cloned();
        let followed = self.playback.read(cx).artist_followed();
        let fans = self.playback.read(cx).artist_fans();
        let albums = self.playback.read(cx).artist_albums().to_vec();
        let simi = self.playback.read(cx).similar_artists().to_vec();
        let np_cards = self.np_cards(cx);
        for card in &np_cards {
            let el: Option<AnyElement> = match card {
                NpCard::Lyrics => {
                    if lyrics.is_empty() {
                        None
                    } else {
                        Some(lyrics_card(&lyrics, state.position_ms))
                    }
                }
                NpCard::Comments => Some(comments_preview_card(&comments, weak.clone())),
                NpCard::SongDetail => song_detail_card(wiki.as_ref(), song.as_ref(), detail.as_ref()),
                NpCard::MusicMemory => wiki
                    .as_ref()
                    .and_then(|w| w.music_memory.clone())
                    .map(|m| music_memory_card(&m)),
                NpCard::AboutArtist => artist
                    .as_ref()
                    .map(|a| about_artist_card(a, followed, fans, &self.services.covers, weak.clone())),
                NpCard::ArtistAlbums => {
                    if albums.is_empty() {
                        None
                    } else {
                        let name = artist.as_ref().map(|a| a.name.clone()).unwrap_or_default();
                        Some(artist_albums_card(
                            &albums,
                            &name,
                            self.np.album_page,
                            &self.services.covers,
                            weak.clone(),
                        ))
                    }
                }
                NpCard::SimilarArtists => {
                    if simi.is_empty() {
                        None
                    } else {
                        Some(similar_artists_card(
                            &simi,
                            self.np.simi_page,
                            &self.services.covers,
                            weak.clone(),
                        ))
                    }
                }
            };
            if let Some(el) = el {
                body = body.child(el);
            }
        }

        div()
            .id("shell-now-playing")
            .v_flex()
            .w(px(np_width))
            .min_w(px(np_width))
            .h_full()
            .flex_none()
            .flex_shrink_0()
            .rounded(px(theme::RADIUS_MD))
            .bg(panel_bg)
            .p(px(spacing::MD))
            .gap(px(spacing::SM))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(spacing::SM))
                    .child(now_playing_menu(weak.clone(), context, cx))
                    .child(div().flex_1().min_w(px(0.0)).child(source_title(source, weak.clone())))
                    .child(toggle_button("np-collapse", IconName::ChevronRight, weak)),
            )
            .child(if SettingsStore::of(cx).np_scrollbar() {
                smooth_scroll("np-scroll", body, window, cx).into_any_element()
            } else {
                smooth_scroll_plain("np-scroll", body, window, cx).into_any_element()
            })
            .into_any_element()
    }
}

/// 头部「…」菜单：心动模式 / 相似歌曲漫游（处于特殊上下文时可退出）。
fn now_playing_menu(weak: WeakEntity<Root>, context: Option<String>, cx: &App) -> impl IntoElement {
    let weak_i = weak.clone();
    let weak_s = weak.clone();
    let weak_e = weak.clone();
    let in_special = context
        .as_deref()
        .map(|c| c == super::super::playback::CONTEXT_INTELLIGENCE || c == super::super::playback::CONTEXT_ROAMING)
        .unwrap_or(false);
    let exit_label = if context.as_deref() == Some(super::super::playback::CONTEXT_ROAMING) {
        "退出相似漫游"
    } else {
        "退出心动模式"
    };
    Button::new("np-more")
        .custom(ButtonCustomVariant::new(cx))
        .icon(IconName::Ellipsis)
        .dropdown_menu(move |menu, _window, _cx| {
            let weak_i = weak_i.clone();
            let weak_s = weak_s.clone();
            let weak_e = weak_e.clone();
            let mut menu = menu;
            if in_special {
                menu = menu.item(PopupMenuItem::new(exit_label).icon(Icon::new(IconName::X)).on_click(
                    move |_ev, _window, cx| {
                        weak_e
                            .update(cx, |root, cx| {
                                root.playback.update(cx, |pb, cx| pb.exit_special_context(cx))
                            })
                            .ok();
                    },
                ));
            }
            menu = menu
                .item(
                    PopupMenuItem::new("心动模式")
                        .icon(Icon::new(IconName::Sparkles))
                        .on_click(move |_ev, _window, cx| {
                            weak_i
                                .update(cx, |root, cx| {
                                    root.playback.update(cx, |pb, cx| pb.start_intelligence(cx))
                                })
                                .ok();
                        }),
                )
                .item(
                    PopupMenuItem::new("相似歌曲漫游")
                        .icon(Icon::new(IconName::Compass))
                        .on_click(move |_ev, _window, cx| {
                            weak_s
                                .update(cx, |root, cx| {
                                    root.playback.update(cx, |pb, cx| pb.play_similar_roaming(cx))
                                })
                                .ok();
                        }),
                );
            menu
        })
}

/// 来源标题：显示播放来源的歌单/专辑名（可点击跳转），否则显示「正在播放」。
fn source_title(source: Option<PlaySource>, weak: WeakEntity<Root>) -> AnyElement {
    let Some(source) = source else {
        return div()
            .text_sm()
            .font_weight(FontWeight::BOLD)
            .text_color(theme::text_primary())
            .truncate()
            .child("正在播放")
            .into_any_element();
    };
    let label = source.name.clone();
    let (is_album, id, name) = (source.kind == PlaySourceKind::Album, source.id, source.name.clone());
    div()
        .id("np-source")
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text_primary())
        .truncate()
        .cursor_pointer()
        .hover(|s| s.text_color(theme::accent()))
        .on_click(move |_ev, _window, cx| {
            weak.update(cx, |root, cx| {
                if is_album {
                    root.open_album(id, name.clone(), cx);
                } else {
                    root.open_playlist(id, name.clone(), cx);
                }
            })
            .ok();
        })
        .child(label)
        .into_any_element()
}

/// 信息卡外壳：圆角底 + 标题 + 内容（对齐 `InfoCard`）。
///
/// `id` 为本卡片的元素 id：卡片流由同一 view 渲染，**每张卡必须自带 id**，
/// 否则卡内交互元素（按钮等）的元素 id 路径会与同帧其它卡片完全相同，
/// 触发 gpui 的 a11y 重复节点断言（debug 构建崩溃）。
fn info_card(id: &'static str, title: &str, content: AnyElement) -> AnyElement {
    div()
        .id(id)
        .v_flex()
        .w_full()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(theme::card_surface())
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

/// 歌词预览卡：当前行附近若干行，当前行高亮。
fn lyrics_card(lines: &[siren_core::lyrics::LyricLine], position_ms: i64) -> AnyElement {
    let idx = siren_core::lyrics::current_index(lines, position_ms).unwrap_or(0);
    let start = idx.saturating_sub(1);
    let end = (idx + 4).min(lines.len());
    let mut col = div().v_flex().w_full().gap(px(6.0)).overflow_hidden();
    for (i, line) in lines.iter().enumerate().skip(start).take(end - start) {
        let active = i == idx;
        let color = if active {
            theme::text_primary()
        } else {
            theme::text_gray()
        };
        let mut item = div()
            .v_flex()
            .w_full()
            .gap(px(spacing::XXS))
            .child(div().w_full().truncate().text_color(color).child(line.text.clone()));
        if let Some(t) = &line.translation {
            item = item.child(
                div()
                    .w_full()
                    .truncate()
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(t.clone()),
            );
        }
        col = col.child(item);
    }
    info_card("np-card-lyrics", "歌词预览", col.into_any_element())
}

/// 评论预览卡（前两条 + 查看全部）。
fn comments_preview_card(page: &siren_core::model::CommentPage, weak: WeakEntity<Root>) -> AnyElement {
    let mut col = div().v_flex().w_full().gap(px(10.0));
    for item in page.comments.iter().take(2) {
        col = col.child(
            div()
                .w_full()
                .v_flex()
                .gap(px(spacing::XS))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child(item.user.nickname.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(item.liked_count.to_string()),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child(item.content.clone()),
                ),
        );
    }
    let weak_open = weak;
    col = col.child(
        div().w_full().flex().justify_start().child(
            Button::new("comments-open")
                .ghost()
                .label(format!("查看全部 {} 条评论", page.total))
                .on_click(move |_ev, _window, cx| {
                    weak_open.update(cx, |root, cx| root.open_comments(cx)).ok();
                }),
        ),
    );
    info_card("np-card-comments", "评论", col.into_any_element())
}

/// 回忆坐标卡。
fn music_memory_card(mem: &siren_core::model::MusicMemory) -> AnyElement {
    let mut text = String::new();
    if !mem.first_listen_date.is_empty() {
        text.push_str(&format!("首次收听 {}", mem.first_listen_date));
        if !mem.season.is_empty() {
            text.push_str(&format!("（{}", mem.season));
            if !mem.period.is_empty() {
                text.push_str(&format!("·{}", mem.period));
            }
            text.push('）');
        }
    }
    if mem.play_count > 0 {
        if !text.is_empty() {
            text.push('；');
        }
        text.push_str(&format!("累计播放 {} 次", mem.play_count));
        if !mem.play_count_text.is_empty() {
            text.push_str(&format!("，{}", mem.play_count_text));
        }
    }
    info_card(
        "np-card-memory",
        "回忆坐标",
        div()
            .w_full()
            .child(div().text_sm().text_color(theme::text_gray()).child(text))
            .into_any_element(),
    )
}

/// 关于歌手卡（头像 + 简介 + 关注）。
fn about_artist_card(
    artist: &siren_core::model::ArtistDetail,
    followed: bool,
    fans: i64,
    covers: &super::super::covers::CoverLoader,
    weak: WeakEntity<Root>,
) -> AnyElement {
    let weak_follow = weak;
    let id = artist.id;
    let mut col = div()
        .flex()
        .items_start()
        .gap(px(10.0))
        .child(
            div()
                .w(px(48.0))
                .h(px(48.0))
                .flex_none()
                .child(covers.image_round(&artist.cover_url, 48.0)),
        )
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(spacing::XS))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_primary())
                        .child(artist.name.clone()),
                )
                .child(div().text_xs().text_color(theme::text_gray()).child(format!(
                    "{} 专辑 · {} 首 · {} 粉丝",
                    artist.album_size, artist.music_size, fans
                )))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child(short(&artist.brief_desc, 60)),
                ),
        );
    col = col.child(
        div()
            .id("artist-follow")
            .flex_none()
            .px(px(10.0))
            .py(px(spacing::XS))
            .rounded_full()
            .cursor_pointer()
            .bg(if followed { theme::surface() } else { theme::accent() })
            .child(div().text_xs().text_color(theme::text_primary()).child(if followed {
                "已关注"
            } else {
                "关注"
            }))
            .on_click(move |_ev, _window, cx| {
                let _ = id;
                weak_follow
                    .update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.toggle_artist_follow(cx))
                    })
                    .ok();
            }),
    );
    info_card("np-card-about-artist", "关于歌手", col.into_any_element())
}

/// 歌手专辑卡（横向列表）。
fn artist_albums_card(
    albums: &[siren_core::model::ArtistAlbum],
    artist_name: &str,
    page: usize,
    covers: &super::super::covers::CoverLoader,
    weak: WeakEntity<Root>,
) -> AnyElement {
    const PER: usize = 3;
    let pages = albums.len().div_ceil(PER).max(1);
    let p = page.min(pages - 1);
    let slice = &albums[(p * PER).min(albums.len())..((p + 1) * PER).min(albums.len())];
    let mut row = div().flex().gap(px(10.0)).w_full();
    for album in slice {
        row = row.child(
            div()
                .v_flex()
                .w(px(108.0))
                .gap(px(6.0))
                .child(covers.image(&album.cover_url, 108.0))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_primary())
                        .truncate()
                        .child(album.name.clone()),
                ),
        );
    }
    let title = if artist_name.is_empty() {
        "更多专辑".to_string()
    } else {
        format!("{artist_name}的更多专辑")
    };
    paged_card(
        "np-albums",
        title,
        p,
        pages,
        |root, delta, cx| {
            let next = root.np.album_page.saturating_add_signed(delta);
            root.set_np_page(true, next, cx);
        },
        row.into_any_element(),
        weak,
    )
}

/// 相似歌手卡（横向分页，左右按钮切换）。
fn similar_artists_card(
    artists: &[siren_core::model::Artist],
    page: usize,
    covers: &super::super::covers::CoverLoader,
    weak: WeakEntity<Root>,
) -> AnyElement {
    const PER: usize = 4;
    let pages = artists.len().div_ceil(PER).max(1);
    let p = page.min(pages - 1);
    let slice = &artists[(p * PER).min(artists.len())..((p + 1) * PER).min(artists.len())];
    let mut row = div().flex().gap(px(10.0)).w_full();
    for artist in slice {
        row = row.child(
            div()
                .v_flex()
                .w(px(72.0))
                .items_center()
                .gap(px(6.0))
                .child(covers.image_round(&artist.cover_url, 64.0))
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(theme::text_primary())
                        .child(artist.name.clone()),
                ),
        );
    }
    paged_card(
        "np-simi",
        "探索类似艺人".to_string(),
        p,
        pages,
        |root, delta, cx| {
            let next = root.np.simi_page.saturating_add_signed(delta);
            root.set_np_page(false, next, cx);
        },
        row.into_any_element(),
        weak,
    )
}

/// 分页信息卡：标题 + 右侧左右翻页箭头 + 内容。
///
/// `card` 为该卡片的唯一标识：翻页按钮的 `ElementId` 由它派生。
/// **不能只用图标作 id**——多张分页卡同帧渲染时元素 id 路径会完全相同，
/// 触发 gpui 的 a11y 重复节点断言（debug 构建直接 panic）。
///
/// `on_page` 收到的增量为 `-1`（上一页）/ `+1`（下一页）。
fn paged_card(
    card: &'static str,
    title: String,
    page: usize,
    page_count: usize,
    on_page: impl Fn(&mut Root, isize, &mut Context<Root>) + Clone + 'static,
    content: AnyElement,
    weak: WeakEntity<Root>,
) -> AnyElement {
    let weak_left = weak.clone();
    let weak_right = weak;
    let left_page = on_page.clone();
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(page_arrow(
                    card,
                    0,
                    IconName::ChevronLeft,
                    page > 0,
                    left_page,
                    weak_left,
                ))
                .child(page_arrow(
                    card,
                    1,
                    IconName::ChevronRight,
                    page + 1 < page_count,
                    on_page,
                    weak_right,
                )),
        );
    div()
        .id(card)
        .v_flex()
        .w_full()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(theme::card_surface())
        .child(header)
        .child(content)
        .into_any_element()
}

fn page_arrow(
    card: &'static str,
    dir: usize,
    name: IconName,
    enabled: bool,
    on_page: impl Fn(&mut Root, isize, &mut Context<Root>) + 'static,
    weak: WeakEntity<Root>,
) -> AnyElement {
    let tooltip = if matches!(name, IconName::ChevronLeft) {
        "上一页"
    } else {
        "下一页"
    };
    Button::new((card, dir))
        .ghost()
        .icon(icon(
            name,
            14.0,
            if enabled {
                theme::text_primary()
            } else {
                theme::text_gray()
            },
        ))
        .tooltip(tooltip)
        .disabled(!enabled)
        .when(enabled, |s| {
            s.on_click(move |_ev, _window, cx| {
                let delta = if dir == 0 { -1 } else { 1 };
                weak.update(cx, |root, cx| on_page(root, delta, cx)).ok();
            })
        })
        .into_any_element()
}

impl Root {
    /// 设置歌手专辑 / 相似歌手卡片页码。
    pub(crate) fn set_np_page(&mut self, album: bool, page: usize, cx: &mut Context<Self>) {
        if album {
            self.np.album_page = page;
        } else {
            self.np.simi_page = page;
        }
        cx.notify();
    }
}

fn short(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// 降低亮度（用于封面取色背景）。
fn darken(color: Rgba, factor: f32) -> Rgba {
    Rgba {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: 1.0,
    }
}

/// 歌曲详情卡：曲风/专辑/语种/发行时间/BPM/影综/获奖/制作人员（缺失项不显示）。
fn song_detail_card(wiki: Option<&SongWiki>, song: Option<&Song>, detail: Option<&Song>) -> Option<AnyElement> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let album = wiki
        .map(|w| w.album.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| song.map(|s| s.album.clone()))
        .unwrap_or_default();
    if let Some(w) = wiki {
        if !w.style.is_empty() {
            rows.push(("曲风".into(), w.style.clone()));
        }
    }
    if !album.is_empty() {
        rows.push(("专辑".into(), album));
    }
    if let Some(w) = wiki {
        if !w.language.is_empty() {
            rows.push(("语种".into(), w.language.clone()));
        }
    }
    let publish = wiki
        .map(|w| w.publish_time.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| detail.filter(|d| d.publish_time > 0).map(|d| fmt_date(d.publish_time)))
        .unwrap_or_default();
    if !publish.is_empty() {
        rows.push(("发行时间".into(), publish));
    }
    if let Some(w) = wiki {
        if !w.bpm.is_empty() {
            rows.push(("BPM".into(), w.bpm.clone()));
        }
        if !w.entertainment.is_empty() {
            rows.push(("影综".into(), w.entertainment.clone()));
        }
        if !w.awards.is_empty() {
            let mut v = w.awards.join(" / ");
            if w.award_total as usize > w.awards.len() {
                v = format!("{v}（共 {} 项）", w.award_total);
            }
            rows.push(("获奖".into(), v));
        }
        if !w.creator_roles.is_empty() {
            let creators = w
                .creator_roles
                .iter()
                .map(|r| format!("{} {}", r.role, r.artists.join(" ")))
                .collect::<Vec<_>>()
                .join(" / ");
            rows.push(("制作人员".into(), creators));
        }
    }
    if rows.is_empty() {
        return None;
    }
    let mut col = div().v_flex().w_full().gap(px(6.0));
    for (label, value) in rows {
        col = col.child(
            div()
                .w_full()
                .flex()
                .items_start()
                .gap(px(spacing::SM))
                .child(
                    div()
                        .w(px(64.0))
                        .flex_none()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_sm()
                        .text_color(theme::text_primary())
                        .child(value),
                ),
        );
    }
    Some(info_card("np-card-song-detail", "歌曲详情", col.into_any_element()))
}

fn toggle_button(id: &'static str, name: IconName, weak: WeakEntity<Root>) -> impl IntoElement {
    let tooltip = if id == "np-expand" {
        "展开正在播放"
    } else {
        "收起正在播放"
    };
    Button::new(id)
        .ghost()
        .icon(icon(name, 16.0, theme::text_gray()))
        .tooltip(tooltip)
        .on_click(move |_ev, _window, cx| {
            weak.update(cx, |root, cx| root.toggle_now_playing(cx)).ok();
        })
}
