//! 音乐库（常驻侧栏）：头部工具栏 + 分栏（歌单 / 专辑 / 艺人）。
//!
//! 对齐 Melodia 的库面板：标题「音乐库」+ 新建/展开；三栏各自列表，登录后加载。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::*;

use siren_core::model::{Album, Artist, Playlist};

use super::super::services::Services;
use super::super::session::{Account, Session};
use super::super::smooth_scroll::smooth_virtual_list;
use super::super::{icon, load_error, PageEvent, PLAYLIST_PAGE};
use super::dim;
use crate::theme::{self, spacing};

/// 音乐库分栏。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryTab {
    Playlist,
    Album,
    Artist,
}

pub(crate) struct LibraryPage {
    services: Services,
    /// 登录表单输入与状态（视图态；账号身份存于 [`Session`]）。
    login_input: Entity<InputState>,
    tab: LibraryTab,
    my_playlists: Arc<Vec<Playlist>>,
    albums: Arc<Vec<Album>>,
    artists: Arc<Vec<Artist>>,
    load_gen: u64,
    albums_gen: u64,
    artists_gen: u64,
    /// 「我的歌单」分页：已加载条数（下一页 offset）与是否可能还有更多（K1）。
    playlists_offset: u32,
    playlists_more: bool,
    /// 各分栏是否已尝试过首次加载（成功或失败）——避免空列表时每次切 tab 重复请求（K8）。
    playlists_loaded: bool,
    albums_loaded: bool,
    artists_loaded: bool,
    /// 各分栏最近一次加载是否失败（除 toast 外在列表区给错误 + 重试）。
    playlists_failed: bool,
    albums_failed: bool,
    artists_failed: bool,
    /// 当前在详情页打开的歌单 id（侧栏当前项选中态）。
    open_playlist: Option<i64>,
    /// 账号刷新代次：logout 时自增，使在途的 account() 响应失效（避免被“登回”）。
    account_gen: u64,
    _login_sub: Subscription,
}

impl EventEmitter<PageEvent> for LibraryPage {}

/// 账号刷新结果：区分有效账号 / 会话过期 / 网络失败。
enum AccountOutcome {
    Ok {
        uid: i64,
        nickname: Option<String>,
        avatar: String,
    },
    Expired,
    Network,
}

impl LibraryPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>, services: Services) -> Self {
        let login_input = cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 MUSIC_U 或完整 Cookie"));
        let _login_sub = cx.subscribe(&login_input, |this: &mut LibraryPage, _i, event: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.do_login(cx);
            }
        });
        let mut page = Self {
            services,
            login_input,
            tab: LibraryTab::Playlist,
            my_playlists: Arc::new(Vec::new()),
            albums: Arc::new(Vec::new()),
            artists: Arc::new(Vec::new()),
            load_gen: 0,
            albums_gen: 0,
            artists_gen: 0,
            playlists_offset: 0,
            playlists_more: false,
            playlists_loaded: false,
            albums_loaded: false,
            artists_loaded: false,
            playlists_failed: false,
            albums_failed: false,
            artists_failed: false,
            open_playlist: None,
            account_gen: 0,
            _login_sub,
        };
        page.refresh_account(false, cx);
        page
    }

    fn do_login(&mut self, cx: &mut Context<Self>) {
        let raw = self.login_input.read(cx).value().trim().to_string();
        let Some(cookie) = siren_core::auth::normalize_cookie(&raw) else {
            self.services.toast_error("请输入 MUSIC_U 或完整 Cookie", cx);
            cx.notify();
            return;
        };
        siren_core::auth::save_cookie(&cookie);
        self.services.client.set_cookie(Some(cookie));
        cx.notify();
        self.refresh_account(true, cx);
    }

    /// 退出登录：清除 Cookie 与会话，回到未登录态。
    pub(crate) fn logout(&mut self, cx: &mut Context<Self>) {
        siren_core::auth::clear_cookie();
        self.services.client.set_cookie(None);
        Session::set(cx, None);
        self.tab = LibraryTab::Playlist;
        self.my_playlists = Arc::new(Vec::new());
        self.albums = Arc::new(Vec::new());
        self.artists = Arc::new(Vec::new());
        // 重置加载/分页标记，下次登录重新拉取。
        self.playlists_offset = 0;
        self.playlists_more = false;
        self.playlists_loaded = false;
        self.albums_loaded = false;
        self.artists_loaded = false;
        self.playlists_failed = false;
        self.albums_failed = false;
        self.artists_failed = false;
        self.open_playlist = None;
        // 使在途的 account() 响应作废，避免退出后被迟到的响应“登回”。
        self.account_gen = self.account_gen.wrapping_add(1);
        cx.emit(PageEvent::LoggedOut);
        cx.notify();
    }

    pub(crate) fn refresh_account(&mut self, announce: bool, cx: &mut Context<Self>) {
        self.account_gen = self.account_gen.wrapping_add(1);
        let gen = self.account_gen;
        let had_cookie = self.services.client.cookie().is_some();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<AccountOutcome>();
        self.services.handle.spawn(async move {
            // 区分「会话过期」与「网络失败」：后者不应清除本地登录态。
            let mut network_err = false;
            let mut account = match siren_core::network::api::account(&client).await {
                Ok(v) => Some(v),
                Err(_) => {
                    network_err = true;
                    None
                }
            };
            // 账号校验失败（未登录/会话过期/网络失败）时尝试续期一次
            if account
                .as_ref()
                .and_then(siren_core::network::api::account_uid)
                .is_none()
            {
                match siren_core::network::api::refresh_login(&client).await {
                    Ok(Some(cookie)) => {
                        client.set_cookie(Some(cookie.clone()));
                        siren_core::auth::save_cookie(&cookie);
                        match siren_core::network::api::account(&client).await {
                            Ok(v) => {
                                account = Some(v);
                                network_err = false;
                            }
                            Err(_) => network_err = true,
                        }
                    }
                    Ok(None) => {}
                    Err(_) => network_err = true,
                }
            }
            let outcome = match account {
                Some(v) if siren_core::network::api::account_uid(&v).is_some() => AccountOutcome::Ok {
                    uid: siren_core::network::api::account_uid(&v).unwrap_or_default(),
                    nickname: siren_core::network::api::account_nickname(&v),
                    avatar: siren_core::network::api::account_avatar(&v).unwrap_or_default(),
                },
                _ if network_err => AccountOutcome::Network,
                _ => AccountOutcome::Expired,
            };
            let _ = tx.send(outcome);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = rx.await.unwrap_or(AccountOutcome::Network);
            this.update(cx, |this, cx| {
                // 代次不符：期间已 logout/重新刷新，丢弃这次响应。
                if this.account_gen != gen {
                    return;
                }
                match outcome {
                    AccountOutcome::Ok { uid, nickname, avatar } => {
                        let nickname: SharedString = nickname.unwrap_or_else(|| "已登录".to_string()).into();
                        Session::set(
                            cx,
                            Some(Account {
                                uid,
                                nickname: nickname.clone(),
                                avatar_url: avatar,
                            }),
                        );
                        log::info!("已登录：{nickname} (uid={uid})");
                        if announce {
                            this.services.toast_success(format!("登录成功：{nickname}"), cx);
                        }
                        cx.emit(PageEvent::SignedIn);
                        this.tab = LibraryTab::Playlist;
                        this.load_my_playlists(cx);
                    }
                    AccountOutcome::Expired => {
                        Session::set(cx, None);
                        if announce {
                            this.services.toast_error("登录失败，请检查 Cookie", cx);
                        } else if had_cookie {
                            this.services.toast_error("登录已过期，请重新登录", cx);
                        }
                        cx.emit(PageEvent::LoggedOut);
                    }
                    AccountOutcome::Network => {
                        // 网络失败：保留本地会话，只提示，不误报“登录已过期”。
                        this.services.toast_error("网络错误，请稍后重试", cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_my_playlists(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = Session::uid(cx) else {
            return;
        };
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::user_playlists(&client, uid, PLAYLIST_PAGE, 0).await);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                this.playlists_loaded = true;
                this.playlists_failed = result.as_ref().map(|r| r.is_err()).unwrap_or(true);
                match result {
                    Some(Ok(list)) => {
                        this.playlists_offset = list.len() as u32;
                        this.playlists_more = list.len() as u32 >= PLAYLIST_PAGE;
                        let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                        this.my_playlists = Arc::new(list);
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 追加下一页「我的歌单」（K1：超过一页时可继续加载，而非硬截断 50 条）。
    fn load_more_playlists(&mut self, cx: &mut Context<Self>) {
        if !self.playlists_more {
            return;
        }
        let Some(uid) = Session::uid(cx) else {
            return;
        };
        let offset = self.playlists_offset;
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::user_playlists(&client, uid, PLAYLIST_PAGE, offset).await);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                match result {
                    Some(Ok(list)) => {
                        this.playlists_offset += list.len() as u32;
                        this.playlists_more = list.len() as u32 >= PLAYLIST_PAGE;
                        let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                        let mut merged: Vec<Playlist> = this.my_playlists.as_ref().clone();
                        merged.extend(list);
                        this.my_playlists = Arc::new(merged);
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 拉取收藏专辑。
    fn load_albums(&mut self, cx: &mut Context<Self>) {
        self.albums_gen = self.albums_gen.wrapping_add(1);
        let gen = self.albums_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::album_sublist(&client).await);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.albums_gen != gen {
                    return;
                }
                this.albums_loaded = true;
                this.albums_failed = result.as_ref().map(|r| r.is_err()).unwrap_or(true);
                match result {
                    Some(Ok(list)) => {
                        let urls: Vec<String> = list.iter().map(|a| a.cover_url.clone()).collect();
                        this.albums = Arc::new(list);
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 拉取收藏歌手。
    fn load_artists(&mut self, cx: &mut Context<Self>) {
        self.artists_gen = self.artists_gen.wrapping_add(1);
        let gen = self.artists_gen;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::artist_sublist(&client).await);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.artists_gen != gen {
                    return;
                }
                this.artists_loaded = true;
                this.artists_failed = result.as_ref().map(|r| r.is_err()).unwrap_or(true);
                match result {
                    Some(Ok(list)) => {
                        let urls: Vec<String> = list.iter().map(|a| a.cover_url.clone()).collect();
                        this.artists = Arc::new(list);
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 重新拉取我的歌单（新建歌单后调用）。
    pub(crate) fn reload_playlists(&mut self, cx: &mut Context<Self>) {
        self.load_my_playlists(cx);
    }

    /// 切换分栏；首次进入按需加载。
    fn select_tab(&mut self, tab: LibraryTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        cx.notify();
        self.ensure_loaded(cx);
    }

    fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        // 以「是否尝试过」而非「列表是否为空」为判据：空列表（真的没有）或加载失败时
        // 不再每次切 tab 重复请求（K8）。
        match self.tab {
            LibraryTab::Playlist => {
                if !self.playlists_loaded {
                    self.load_my_playlists(cx);
                }
            }
            LibraryTab::Album => {
                if !self.albums_loaded {
                    self.load_albums(cx);
                }
            }
            LibraryTab::Artist => {
                if !self.artists_loaded {
                    self.load_artists(cx);
                }
            }
        }
    }

    fn header(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        let weak_add = weak;
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(spacing::XXS))
            .pb(px(10.0))
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text_primary())
                    .child("音乐库"),
            )
            .child(
                div().flex().items_center().gap(px(spacing::XXS)).child(
                    div()
                        .id("lib-add")
                        .size(px(30.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme::surface())
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::surface_light()))
                        .on_click(move |_ev, _window, cx| {
                            weak_add.update(cx, |_p, cx| cx.emit(PageEvent::OpenCreate)).ok();
                        })
                        .child(icon(IconName::Plus, 18.0, theme::text_gray())),
                ),
            )
    }

    fn tabs(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::XXS))
            .pb(px(spacing::XXS))
            .child(lib_tab(
                "歌单",
                self.tab == LibraryTab::Playlist,
                weak.clone(),
                LibraryTab::Playlist,
            ))
            .child(lib_tab(
                "专辑",
                self.tab == LibraryTab::Album,
                weak.clone(),
                LibraryTab::Album,
            ))
            .child(lib_tab(
                "艺人",
                self.tab == LibraryTab::Artist,
                weak,
                LibraryTab::Artist,
            ))
    }

    /// 打开歌单：记下当前项（侧栏选中态），并从「专辑/艺人」分栏自动切回「歌单」。
    fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.open_playlist = Some(id);
        self.tab = LibraryTab::Playlist;
        cx.emit(PageEvent::OpenPlaylist { id, name });
        cx.notify();
    }

    /// 收藏专辑列表。
    fn album_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.albums_failed {
            return load_error(
                "lib-albums-retry",
                cx.weak_entity(),
                "收藏专辑加载失败",
                LibraryPage::load_albums,
            );
        }
        if !self.albums_loaded {
            return list_hint("加载中…");
        }
        if self.albums.is_empty() {
            return list_hint("暂无收藏专辑");
        }
        let albums = self.albums.clone();
        let count = albums.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "lib-albums",
            count,
            dim::ROW_H_LIST,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let album = &albums[i];
                        let cover = covers.image(&album.cover_url, dim::COVER_LIST);
                        let weak = weak.clone();
                        let (id, name) = (album.id, album.name.clone());
                        div()
                            .id(format!("lib-album-{id}"))
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
                                weak.update(cx, |_page, cx| {
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
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_gray())
                                            .truncate()
                                            .child(album.artists.clone()),
                                    ),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
            window,
            cx,
        )
        .into_any_element()
    }

    /// 收藏歌手列表。
    fn artist_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.artists_failed {
            return load_error(
                "lib-artists-retry",
                cx.weak_entity(),
                "关注歌手加载失败",
                LibraryPage::load_artists,
            );
        }
        if !self.artists_loaded {
            return list_hint("加载中…");
        }
        if self.artists.is_empty() {
            return list_hint("暂无关注歌手");
        }
        let artists = self.artists.clone();
        let count = artists.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "lib-artists",
            count,
            dim::ROW_H_LIST,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let artist = &artists[i];
                        let cover = covers.image_round(&artist.cover_url, dim::COVER_SONG);
                        let weak = weak.clone();
                        let (id, name) = (artist.id, artist.name.clone());
                        div()
                            .id(format!("lib-artist-{id}"))
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
                                weak.update(cx, |_page, cx| {
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
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
            window,
            cx,
        )
        .into_any_element()
    }

    /// 置顶 / 取消置顶自建歌单。
    fn pin_playlist(&mut self, id: i64, top: bool, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::playlist_top(&client, id, top).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(())) => {
                        this.services
                            .toast_success(if top { "已置顶歌单" } else { "已取消置顶" }, cx);
                        this.load_my_playlists(cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 上/下移歌单并同步云端顺序。
    fn move_playlist(&mut self, id: i64, dir: i32, cx: &mut Context<Self>) {
        let Some(i) = self.my_playlists.iter().position(|p| p.id == id) else {
            return;
        };
        let j = i as i32 + dir;
        if j < 0 || j as usize >= self.my_playlists.len() {
            return;
        }
        let j = j as usize;
        let mut items: Vec<Playlist> = self.my_playlists.as_ref().clone();
        items.swap(i, j);
        let ids: Vec<i64> = items.iter().map(|p| p.id).collect();
        self.my_playlists = Arc::new(items);
        cx.notify();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_order_update(&client, &ids)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            if !ok {
                // 云端保存失败则回滚本地顺序并提示，避免显示与云端不一致。
                this.update(cx, |this, cx| {
                    let mut items: Vec<Playlist> = this.my_playlists.as_ref().clone();
                    items.swap(i, j);
                    this.my_playlists = Arc::new(items);
                    this.services.toast_error("排序保存失败，请重试", cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// 我的歌单列表。
    fn playlist_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.playlists_failed {
            return load_error(
                "lib-playlists-retry",
                cx.weak_entity(),
                "我的歌单加载失败",
                LibraryPage::load_my_playlists,
            );
        }
        if !self.playlists_loaded {
            return list_hint("加载中…");
        }
        if self.my_playlists.is_empty() {
            return list_hint("还没有歌单，点击右上角 + 新建");
        }
        let playlists = self.my_playlists.clone();
        let count = playlists.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let open_playlist = self.open_playlist;
        let list = smooth_virtual_list(
            "lib-playlists",
            count,
            dim::ROW_H_LIST,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let pl = &playlists[i];
                        let cover = covers.image(&pl.cover_url, dim::COVER_LIST);
                        let weak = weak.clone();
                        let weak_ctx = weak.clone();
                        let (id, name) = (pl.id, pl.name.clone());
                        // 当前打开的歌单：surface_light 选中填充。
                        let selected = open_playlist == Some(id);
                        div()
                            .id(format!("lib-playlist-{id}"))
                            .w_full()
                            .h(px(dim::ROW_H_LIST))
                            .px(px(spacing::XXS))
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(if selected {
                                theme::surface_light()
                            } else {
                                rgba(0x00000000)
                            })
                            .hover(|s| s.bg(theme::card_surface()))
                            .cursor_pointer()
                            .on_click(move |_ev, _window, cx| {
                                weak.update(cx, |page, cx| page.open_playlist(id, name.clone(), cx))
                                    .ok();
                            })
                            .context_menu(move |menu, _w, _cx| {
                                let w_pin = weak_ctx.clone();
                                let w_unpin = weak_ctx.clone();
                                let w_up = weak_ctx.clone();
                                let w_down = weak_ctx.clone();
                                menu.item(PopupMenuItem::new("置顶").on_click(move |_e, _w, cx| {
                                    w_pin.update(cx, |page, cx| page.pin_playlist(id, true, cx)).ok();
                                }))
                                .item(PopupMenuItem::new("取消置顶").on_click(move |_e, _w, cx| {
                                    w_unpin.update(cx, |page, cx| page.pin_playlist(id, false, cx)).ok();
                                }))
                                .item(PopupMenuItem::new("上移").on_click(move |_e, _w, cx| {
                                    w_up.update(cx, |page, cx| page.move_playlist(id, -1, cx)).ok();
                                }))
                                .item(PopupMenuItem::new("下移").on_click(move |_e, _w, cx| {
                                    w_down.update(cx, |page, cx| page.move_playlist(id, 1, cx)).ok();
                                }))
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
            },
            window,
            cx,
        );
        let mut col = div().v_flex().flex_1().min_h(px(0.0)).w_full().child(list);
        if self.playlists_more {
            let weak_more = cx.weak_entity();
            col = col.child(
                div()
                    .id("lib-playlists-more")
                    .w_full()
                    .py(px(spacing::SM))
                    .flex()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::card_surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_more.update(cx, |page, cx| page.load_more_playlists(cx)).ok();
                    })
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .hover(|s| s.text_color(theme::accent()))
                            .child("加载更多"),
                    ),
            );
        }
        col.into_any_element()
    }
}

impl Render for LibraryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = Session::account(cx).is_some();
        let content: AnyElement = if !logged_in {
            // 未登录：空态 + 登录引导（登录入口在标题栏账号按钮）。
            div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(spacing::SM))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child("登录后查看你的音乐库"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_gray())
                        .child("点击标题栏账号按钮登录"),
                )
                .into_any_element()
        } else {
            match self.tab {
                LibraryTab::Playlist => self.playlist_list(window, cx),
                LibraryTab::Album => self.album_list(window, cx),
                LibraryTab::Artist => self.artist_list(window, cx),
            }
        };

        div()
            .v_flex()
            .size_full()
            .rounded(px(theme::RADIUS_MD))
            .bg(theme::pane())
            .p(px(spacing::MD))
            .gap(px(spacing::XXS))
            .child(self.header(cx.weak_entity()))
            .child(self.tabs(cx.weak_entity()))
            .child(content)
    }
}

/// 列表区占位文案（加载中 / 空态）。
fn list_hint(text: &str) -> AnyElement {
    div()
        .w_full()
        .py(px(spacing::XL))
        .flex()
        .justify_center()
        .child(div().text_sm().text_color(theme::text_gray()).child(text.to_string()))
        .into_any_element()
}

fn lib_tab(label: &str, active: bool, weak: WeakEntity<LibraryPage>, tab: LibraryTab) -> AnyElement {
    div()
        .id(format!("lib-tab-{label}"))
        .px(px(18.0))
        .h(px(38.0))
        .flex()
        .items_center()
        .rounded_full()
        .cursor_pointer()
        .bg(if active {
            theme::surface_light()
        } else {
            theme::surface()
        })
        .on_click(move |_ev, _window, cx| {
            weak.update(cx, |page, cx| page.select_tab(tab, cx)).ok();
        })
        .child(
            div()
                .text_sm()
                .text_color(theme::text_primary())
                .child(label.to_string()),
        )
        .into_any_element()
}
