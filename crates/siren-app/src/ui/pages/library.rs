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
use super::super::{icon, PageEvent};
use crate::theme;

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
    /// 账号刷新代次：logout 时自增，使在途的 account() 响应失效（避免被“登回”）。
    account_gen: u64,
    _login_sub: Subscription,
}

impl EventEmitter<PageEvent> for LibraryPage {}

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
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let mut account = siren_core::network::api::account(&client).await.ok();
            // 账号校验失败（未登录/会话过期）时尝试续期一次
            if account
                .as_ref()
                .and_then(siren_core::network::api::account_uid)
                .is_none()
            {
                if let Ok(Some(cookie)) = siren_core::network::api::refresh_login(&client).await {
                    client.set_cookie(Some(cookie.clone()));
                    siren_core::auth::save_cookie(&cookie);
                    account = siren_core::network::api::account(&client).await.ok();
                }
            }
            let _ = tx.send(account);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let account = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                // 代次不符：期间已 logout/重新刷新，丢弃这次响应。
                if this.account_gen != gen {
                    return;
                }
                let avatar = account
                    .as_ref()
                    .and_then(siren_core::network::api::account_avatar)
                    .unwrap_or_default();
                let profile = account.and_then(|v| {
                    siren_core::network::api::account_uid(&v)
                        .map(|uid| (uid, siren_core::network::api::account_nickname(&v)))
                });
                match profile {
                    Some((uid, nick)) => {
                        let nickname: SharedString = nick.unwrap_or_else(|| "已登录".to_string()).into();
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
                    None => {
                        Session::set(cx, None);
                        if announce {
                            this.services.toast_error("登录失败，请检查 Cookie", cx);
                        } else if had_cookie {
                            this.services.toast_error("登录已过期，请重新登录", cx);
                        }
                        cx.emit(PageEvent::LoggedOut);
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
            let list = siren_core::network::api::user_playlists(&client, uid, 50, 0)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.my_playlists = Arc::new(list);
                this.services.covers.prefetch(urls, 48.0, cx);
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
        match self.tab {
            LibraryTab::Playlist => {
                if self.my_playlists.is_empty() {
                    self.load_my_playlists(cx);
                }
            }
            LibraryTab::Album => {
                if self.albums.is_empty() {
                    self.load_albums(cx);
                }
            }
            LibraryTab::Artist => {
                if self.artists.is_empty() {
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
            .px(px(6.0))
            .pb(px(10.0))
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text_primary())
                    .child("音乐库"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
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
                    )
                    .child(round_button("lib-expand", IconName::Maximize2)),
            )
    }

    fn tabs(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pb(px(6.0))
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

    fn toolbar(&self) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(6.0))
            .py(px(6.0))
            .child(icon(IconName::Search, 16.0, theme::text_gray()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(div().text_xs().text_color(theme::text_gray()).child("默认"))
                    .child(icon(IconName::List, 16.0, theme::text_gray())),
            )
    }

    /// 收藏专辑列表。
    fn album_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let albums = self.albums.clone();
        let count = albums.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "lib-albums",
            count,
            56.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let album = &albums[i];
                        let cover = covers.image(&album.cover_url, 48.0);
                        let weak = weak.clone();
                        let (id, name) = (album.id, album.name.clone());
                        div()
                            .id(format!("lib-album-{id}"))
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
        let artists = self.artists.clone();
        let count = artists.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "lib-artists",
            count,
            56.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let artist = &artists[i];
                        let cover = covers.image_round(&artist.cover_url, 40.0);
                        let weak = weak.clone();
                        let (id, name) = (artist.id, artist.name.clone());
                        div()
                            .id(format!("lib-artist-{id}"))
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
        let mut items: Vec<Playlist> = self.my_playlists.as_ref().clone();
        items.swap(i, j as usize);
        let ids: Vec<i64> = items.iter().map(|p| p.id).collect();
        self.my_playlists = Arc::new(items);
        cx.notify();
        let client = self.services.client.clone();
        self.services.handle.spawn(async move {
            let _ = siren_core::network::api::playlist_order_update(&client, &ids).await;
        });
    }

    /// 我的歌单列表。
    fn playlist_list(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let playlists = self.my_playlists.clone();
        let count = playlists.len();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        smooth_virtual_list(
            "lib-playlists",
            count,
            56.0,
            move |range, _window, _cx| {
                range
                    .map(|i| {
                        let pl = &playlists[i];
                        let cover = covers.image(&pl.cover_url, 48.0);
                        let weak = weak.clone();
                        let weak_ctx = weak.clone();
                        let (id, name) = (pl.id, pl.name.clone());
                        div()
                            .id(format!("lib-playlist-{id}"))
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
                                weak.update(cx, |_page, cx| {
                                    cx.emit(PageEvent::OpenPlaylist { id, name: name.clone() });
                                })
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
        )
        .into_any_element()
    }
}

impl Render for LibraryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = Session::account(cx).is_some();
        let content: AnyElement = if !logged_in {
            // 未登录：不显示提示卡片（登录入口在标题栏账号按钮）
            div().flex_1().w_full().into_any_element()
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
            .rounded(px(8.0))
            .bg(theme::pane())
            .p(px(12.0))
            .gap(px(2.0))
            .child(self.header(cx.weak_entity()))
            .child(self.tabs(cx.weak_entity()))
            .child(self.toolbar())
            .child(content)
    }
}

fn round_button(id: &'static str, name: IconName) -> impl IntoElement {
    div()
        .id(id)
        .size(px(34.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::surface())
        .cursor_pointer()
        .hover(|s| s.bg(theme::surface_light()))
        .child(icon(name, 18.0, theme::text_primary()))
}

fn lib_tab(label: &str, active: bool, weak: WeakEntity<LibraryPage>, tab: LibraryTab) -> AnyElement {
    div()
        .id(format!("lib-tab-{label}"))
        .px(px(18.0))
        .h(px(38.0))
        .flex()
        .items_center()
        .rounded(px(19.0))
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
