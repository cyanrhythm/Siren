//! 歌曲行：歌单 / 专辑 / 歌手 / 最近播放 / 搜索 5 处共用的「行外壳 + 单元格 + 右键菜单」。
//!
//! 这 5 处的行结构逐字相同、差异固定，散在 5 个页面里改一处就得跟着改 5 处；
//! 抽到这里后各页只声明自己的差异（列集合、行高、内边距、当前曲高亮、点击语义）。
//!
//! 两条硬约束落在本模块里：
//!
//! 1. **元素 id** 取 `{id_prefix}-{song_id}`（前缀沿用各页原有的 `playlist-track-` 等）：
//!    循环项必须含条目唯一键，同一 view 内 id 重复会触发 gpui 的 a11y 断言（docs/10 §7）。
//!    `id` 与 `group` 同串——红心靠 `group` 做悬停显隐（见 [`like_button`]）。
//! 2. **点击语义不在这里猜**：歌单页播「过滤后的曲目」、专辑页播全碟、歌手页播热门、
//!    最近播放播「最近列表」、搜索播「搜索结果」，故回调由调用方传入，行本身不持有列表。

use gpui_kit::base::StyledExt;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::Song;

use super::super::covers::CoverLoader;
use super::super::pages::dim;
use super::super::playback::Playback;
use super::super::{fmt_time, PageEvent};
use super::like_button;
use crate::theme;

/// 追加菜单项：拿到的菜单已含共享项，返回值再挂回行上；
/// 形参与 `ContextMenuExt::context_menu` 的闭包一致（歌单页用它补「从歌单移除」）。
type ExtraMenuItems = Box<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static>;

/// 行点击：各页播放语义不同（过滤后的曲目 / 全碟 / 热门 / 最近列表 / 搜索结果），由调用方给出。
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// 序号列：定宽右对齐槽 + 内容。槽宽、字号、播放中标记各页不同，故差异显式落在字段上。
pub(crate) struct IndexCell {
    /// 槽宽：歌单 / 专辑 / 搜索用 `dim::INDEX_W`，歌手热门用 24（该页行更紧）。
    width: f32,
    /// 字号：歌手热门用 `text_xs`，其余用 `text_sm`。
    small: bool,
    text: SharedString,
    color: Rgba,
}

impl IndexCell {
    /// 灰字序号：`i` 为 0 起下标，显示 `i + 1`（歌单 / 专辑 / 歌手热门）。
    pub(crate) fn number(i: usize, width: f32, small: bool) -> Self {
        Self {
            width,
            small,
            text: format!("{}", i + 1).into(),
            color: theme::text_gray(),
        }
    }

    /// 搜索页序号：播放中显示 ▶（强调色），否则显示序号——选中行强调色、其余灰字。
    pub(crate) fn search(i: usize, current: bool, selected: bool) -> Self {
        let (text, color) = if current {
            ("▶".to_string(), theme::accent())
        } else if selected {
            (format!("{}", i + 1), theme::accent())
        } else {
            (format!("{}", i + 1), theme::text_gray())
        };
        Self {
            width: dim::INDEX_W,
            small: false,
            text: text.into(),
            color,
        }
    }
}

/// 一行的配置。默认值取最紧凑的一条（无序号、无专辑列、有艺人行、有曲长列），
/// 各页按需覆盖；`new` 里那几个字段是「行之所以是这一行」的必需项，故不做成可选项。
pub(crate) struct SongRow<'a, P> {
    id_prefix: &'static str,
    song: &'a Song,
    playback: Entity<Playback>,
    covers: &'a CoverLoader,
    /// 页面弱引用：右键菜单的「添加到歌单」由页面 emit
    /// （5 个页面都实现了 `EventEmitter<PageEvent>`，故这里用泛型而不是回调包一层）。
    page: WeakEntity<P>,
    cx: &'a App,
    on_click: ClickHandler,

    row_h: f32,
    pad: f32,
    index: Option<IndexCell>,
    show_album: bool,
    show_artists: bool,
    show_duration: bool,
    /// 时长列文本；`None` 用曲长。[`SongRow::duration_text`] 用于最近播放页
    /// ——那一列放的是**播放时刻**，与曲长无关。
    duration_text: Option<SharedString>,
    current: bool,
    add_to_playlist: bool,
    extra_menu: Option<ExtraMenuItems>,
}

impl<'a, P: EventEmitter<PageEvent> + 'static> SongRow<'a, P> {
    /// `on_click` 是行的播放语义（各页不同），必填——没有它这一行就只是张图片。
    pub(crate) fn new(
        id_prefix: &'static str,
        song: &'a Song,
        playback: &Entity<Playback>,
        covers: &'a CoverLoader,
        page: WeakEntity<P>,
        cx: &'a App,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id_prefix,
            song,
            playback: playback.clone(),
            covers,
            page,
            cx,
            on_click: Box::new(on_click),
            row_h: dim::ROW_H,
            pad: theme::spacing::LG,
            index: None,
            show_album: false,
            show_artists: true,
            show_duration: true,
            duration_text: None,
            current: false,
            add_to_playlist: false,
            extra_menu: None,
        }
    }

    /// 行高（`dim::ROW_H` / `dim::ROW_H_SM`）。
    pub(crate) fn row_h(mut self, row_h: f32) -> Self {
        self.row_h = row_h;
        self
    }

    /// 行水平内边距，需与同页 `column_header` 的 `pad` 一致才对得齐列。
    pub(crate) fn pad(mut self, pad: f32) -> Self {
        self.pad = pad;
        self
    }

    /// 序号列；不设则无该列（最近播放页）。
    pub(crate) fn index(mut self, cell: IndexCell) -> Self {
        self.index = Some(cell);
        self
    }

    /// 专辑列（仅歌单页有）。
    pub(crate) fn show_album(mut self, show: bool) -> Self {
        self.show_album = show;
        self
    }

    /// 艺人行：歌手热门页的歌都出自该歌手，只显示歌名。
    pub(crate) fn show_artists(mut self, show: bool) -> Self {
        self.show_artists = show;
        self
    }

    /// 时长列；搜索页没有这一列。
    pub(crate) fn show_duration(mut self, show: bool) -> Self {
        self.show_duration = show;
        self
    }

    /// 覆盖时长列文本（最近播放页显示播放时刻）。
    pub(crate) fn duration_text(mut self, text: impl Into<SharedString>) -> Self {
        self.duration_text = Some(text.into());
        self
    }

    /// 当前播放曲：歌单 / 专辑页据此把标题染成强调色。
    /// 搜索页的「播放中」标记在序号列里（见 [`IndexCell::search`]），标题仍用常规色。
    pub(crate) fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    /// 在共享菜单后追加「添加到歌单」（歌单 / 最近播放 / 搜索页有，专辑 / 歌手页没有）。
    pub(crate) fn add_to_playlist(mut self, show: bool) -> Self {
        self.add_to_playlist = show;
        self
    }

    /// 追加本页专有的菜单项（歌单页的「从歌单移除」）。
    pub(crate) fn extra_menu(
        mut self,
        f: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> Self {
        self.extra_menu = Some(Box::new(f));
        self
    }
}

/// 歌手列：多歌手**逐个可点**（`artists` 与 `artist_ids` 数量一致时），
/// 否则退化为整段一个链接（有 id）或纯文本。
///
/// 行本身是「点击播放」，故每个链接都要 `stop_propagation`，否则点歌手会连播。
fn artists_cell<P: EventEmitter<PageEvent> + 'static>(row_id: &str, song: &Song, page: &WeakEntity<P>) -> AnyElement {
    let names: Vec<&str> = song.artists.split('/').filter(|name| !name.is_empty()).collect();
    if names.is_empty() {
        return div().into_any_element();
    }
    let aligned = song.artist_ids.len() == names.len();
    let mut row = div()
        .flex()
        .items_center()
        .min_w(px(0.0))
        .overflow_hidden()
        .text_xs()
        .text_color(theme::text_gray());
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            row = row.child(div().px(px(2.0)).child("/"));
        }
        // 数量不一致时只给第一个歌手挂链接：宁可少挂，也不要挂错人。
        let id = if aligned {
            song.artist_ids.get(i).copied()
        } else if i == 0 {
            song.artist_ids.first().copied()
        } else {
            None
        };
        let segment = div().id(format!("{row_id}-artist-{i}")).child(name.to_string());
        row = row.child(match id {
            Some(id) => {
                let page = page.clone();
                let name = name.to_string();
                segment
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::accent()))
                    .on_click(move |_ev, _w, cx| {
                        page.update(cx, |_p, cx| cx.emit(PageEvent::OpenArtist { id, name: name.clone() }))
                            .ok();
                        cx.stop_propagation();
                    })
                    .into_any_element()
            }
            None => segment.into_any_element(),
        });
    }
    row.into_any_element()
}

/// 渲染一行歌曲，配置见 [`SongRow`]。
pub(crate) fn song_row<P: EventEmitter<PageEvent> + 'static>(cfg: SongRow<'_, P>) -> AnyElement {
    let SongRow {
        id_prefix,
        song,
        playback,
        covers,
        page,
        cx,
        on_click,
        row_h,
        pad,
        index,
        show_album,
        show_artists,
        show_duration,
        duration_text,
        current,
        add_to_playlist,
        extra_menu,
    } = cfg;

    let row_id = format!("{id_prefix}-{}", song.id);
    // 喜欢态只读一次：红心与菜单里的「喜欢 / 取消喜欢」文案同源，不会出现两处不一致。
    let liked = playback.read(cx).is_liked(song.id);

    let mut row = div()
        .id(row_id.clone())
        .group(row_id.clone())
        .w_full()
        .h(px(row_h))
        .px(px(pad))
        .flex()
        .items_center()
        .gap(px(theme::spacing::MD))
        .rounded(px(theme::RADIUS_SM))
        .hover(|s| s.bg(theme::card_surface()))
        .cursor_pointer()
        .on_click(move |ev, window, cx| on_click(ev, window, cx));

    if let Some(cell) = index {
        let mut slot = div().w(px(cell.width)).flex().justify_end().text_color(cell.color);
        slot = if cell.small { slot.text_xs() } else { slot.text_sm() };
        row = row.child(slot.child(cell.text));
    }

    row = row.child(covers.image(&song.cover_url, dim::COVER_SONG));

    let title = div()
        .text_sm()
        .text_color(if current {
            theme::accent()
        } else {
            theme::text_primary()
        })
        .truncate()
        .child(song.name.clone());
    row = row.child(if show_artists {
        div()
            .v_flex()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(title)
            .child(artists_cell(&row_id, song, &page))
    } else {
        // 歌手热门页的标题块是**行方向**单子元素（原代码没写 `v_flex`），
        // 与其它页的列方向双行块不同：列方向会把标题拉伸到整列宽，
        // 长歌名的截断点随之改变，故这里按原样保留两条分支。
        div().flex_1().min_w(px(0.0)).overflow_hidden().child(title)
    });

    if show_album {
        let cell = div()
            .w(px(dim::ALBUM_W))
            .text_xs()
            .text_color(theme::text_gray())
            .truncate();
        // 有专辑 ID 时专辑名可点（跳专辑页），且不触发整行的播放。
        row = row.child(match (song.album_id, song.album.is_empty()) {
            (Some(id), false) => {
                let page = page.clone();
                let album = song.album.clone();
                cell.id(format!("{row_id}-album"))
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::accent()))
                    .on_click(move |_ev, _w, cx| {
                        page.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenAlbum {
                                id,
                                name: album.clone(),
                            })
                        })
                        .ok();
                        cx.stop_propagation();
                    })
                    .child(song.album.clone())
                    .into_any_element()
            }
            _ => cell.child(song.album.clone()).into_any_element(),
        });
    }

    // 红心复用现成组件：悬停显隐、阻止冒泡都在它内部，`group` 传与行同串的 id。
    row = row.child(like_button(&playback, song.clone(), row_id.clone(), cx));

    if show_duration {
        row = row.child(
            div()
                .w(px(dim::DURATION_W))
                .flex()
                .justify_end()
                .text_xs()
                .text_color(theme::text_gray())
                .child(duration_text.unwrap_or_else(|| fmt_time(song.duration_ms).into())),
        );
    }

    // 菜单项在打开时才构建，故把歌与各句柄 clone 进闭包；`liked` 同样取渲染时的快照
    //（与改动前一致：菜单开着时红心状态变化不会改写已展开的菜单文案）。
    let menu_song = song.clone();
    let menu_playback = playback.clone();
    let menu_page = page.clone();
    row.context_menu(move |menu, window, menu_cx| {
        let mut menu = menu
            .item(PopupMenuItem::new("下一首播放").on_click({
                let playback = menu_playback.clone();
                let song = menu_song.clone();
                move |_ev, _w, cx| {
                    playback.update(cx, |pb, cx| pb.play_next(song.clone(), cx));
                }
            }))
            .item(PopupMenuItem::new(if liked { "取消喜欢" } else { "喜欢" }).on_click({
                let playback = menu_playback.clone();
                let song = menu_song.clone();
                move |_ev, _w, cx| {
                    playback.update(cx, |pb, cx| pb.toggle_like_song(song.clone(), cx));
                }
            }));
        // 查看歌手 / 查看专辑：与「点歌手名」等价的鼠标路径（数据齐全时才出现）。
        if let Some(artist_id) = menu_song.artist_ids.first().copied() {
            let page = menu_page.clone();
            let name = menu_song.artists.clone();
            menu = menu.item(PopupMenuItem::new("查看歌手").on_click(move |_ev, _w, cx| {
                page.update(cx, |_p, cx| {
                    cx.emit(PageEvent::OpenArtist {
                        id: artist_id,
                        name: name.clone(),
                    })
                })
                .ok();
            }));
        }
        if let (Some(album_id), false) = (menu_song.album_id, menu_song.album.is_empty()) {
            let page = menu_page.clone();
            let name = menu_song.album.clone();
            menu = menu.item(PopupMenuItem::new("查看专辑").on_click(move |_ev, _w, cx| {
                page.update(cx, |_p, cx| {
                    cx.emit(PageEvent::OpenAlbum {
                        id: album_id,
                        name: name.clone(),
                    })
                })
                .ok();
            }));
        }
        // 复制网页链接（网易云分享用）。
        let link = format!("https://music.163.com/song?id={}", menu_song.id);
        menu = menu.item(PopupMenuItem::new("复制链接").on_click(move |_ev, _w, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
        }));
        if add_to_playlist {
            let page = menu_page.clone();
            let song_id = menu_song.id;
            menu = menu.item(PopupMenuItem::new("添加到歌单").on_click(move |_ev, _w, cx| {
                page.update(cx, |_page, cx| cx.emit(PageEvent::AddToPlaylist { song_id }))
                    .ok();
            }));
        }
        if let Some(extra) = extra_menu.as_deref() {
            menu = extra(menu, window, menu_cx);
        }
        menu
    })
    .into_any_element()
}
