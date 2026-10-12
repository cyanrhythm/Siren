//! GPUI 界面。
//!
//! 多页面骨架：侧栏导航 + 首页（推荐歌单）/ 音乐库 / 搜索 / 歌单详情 + 播放条。
//!
//! ## 模块组织
//!
//! - [`pages`]：页面级视图（首页、音乐库、搜索、歌单详情）
//! - [`components`]：可复用组件（侧栏、播放条）
//! - [`covers`]：封面加载器
//! - [`playback`]：共享播放服务（`Entity<Playback>`）
//!
//! `Root` 只负责导航、页面数据与外壳编排；播放逻辑集中在 [`Playback`]。

pub mod components;
pub mod covers;
pub mod pages;
mod playback;

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Icon;
use gpui_kit::*;

use siren_core::model::{Playlist, PlaylistDetail, Song};
use siren_core::network::client::NeteaseClient;

use crate::theme;
use covers::CoverLoader;
use playback::Playback;

/// 当前页面。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Home,
    Library,
    Search,
    Playlist { id: i64, name: String },
}

/// 主窗口根组件：导航、页面数据与外壳。
pub struct Root {
    runtime: tokio::runtime::Runtime,
    client: Arc<NeteaseClient>,
    covers: CoverLoader,
    playback: Entity<Playback>,
    nav: Vec<Route>,

    // 首页/音乐库
    playlists: Arc<Vec<Playlist>>,
    playlists_status: SharedString,

    // 搜索
    search_input: Entity<InputState>,
    songs: Arc<Vec<Song>>,
    selected: Option<usize>,
    search_status: SharedString,

    // 歌单详情
    playlist: Option<Arc<PlaylistDetail>>,

    // 音乐库 / 登录
    logged_in: bool,
    nickname: SharedString,
    login_input: Entity<InputState>,
    login_status: SharedString,
    my_playlists: Arc<Vec<Playlist>>,
    uid: Option<i64>,

    _search_sub: Subscription,
    _login_sub: Subscription,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, runtime: tokio::runtime::Runtime) -> Entity<Self> {
        siren_platform::prepare_native_environment();
        let cookie = std::env::var("SIREN_COOKIE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| siren_core::auth::normalize_cookie(&s))
            .or_else(siren_core::auth::load_cookie);
        let has_cookie = cookie.is_some();
        let client = Arc::new(NeteaseClient::with_cookie(cookie));
        let covers = CoverLoader::new(client.clone(), runtime.handle().clone());
        let playback = Playback::new(window, cx, client.clone(), runtime.handle().clone());

        let initial_route = match std::env::var("SIREN_START").ok().as_deref() {
            Some("library") => Route::Library,
            Some("search") => Route::Search,
            _ => Route::Home,
        };

        let entity = cx.new(|cx| {
            let search_input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("搜索歌曲、歌手…")
                    .default_value("周杰伦")
            });
            let _search_sub = cx.subscribe(&search_input, |this: &mut Root, _input, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.start_search(cx);
                }
            });

            let login_input = cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 MUSIC_U 或完整 Cookie"));
            let _login_sub = cx.subscribe(&login_input, |this: &mut Root, _input, event: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.do_login(cx);
                }
            });

            Self {
                runtime,
                client,
                covers,
                playback,
                nav: vec![initial_route],
                playlists: Arc::new(Vec::new()),
                playlists_status: "加载中…".into(),
                search_input,
                songs: Arc::new(Vec::new()),
                selected: None,
                search_status: "回车搜索".into(),
                playlist: None,
                logged_in: false,
                nickname: "未登录".into(),
                login_input,
                login_status: if has_cookie {
                    "登录校验中…".into()
                } else {
                    "未登录".into()
                },
                my_playlists: Arc::new(Vec::new()),
                uid: None,
                _search_sub,
                _login_sub,
            }
        });

        entity.update(cx, |this, cx| {
            this.load_playlists(cx);
            if has_cookie {
                this.refresh_account(cx);
            }
        });
        entity
    }

    fn route(&self) -> Route {
        self.nav.last().cloned().unwrap_or(Route::Home)
    }

    fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        self.nav.truncate(1);
        if let Route::Home = route {
        } else {
            self.nav.push(route);
        }
        if matches!(self.route(), Route::Home) && self.playlists.is_empty() {
            self.load_playlists(cx);
        }
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.len() > 1 {
            self.nav.pop();
            cx.notify();
        }
    }

    // ---------- 数据加载 ----------

    fn load_playlists(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let list = siren_core::network::api::personalized_playlists(&client, 30)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        self.playlists_status = "加载推荐歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.playlists_status = format!("推荐歌单 {} 个", list.len()).into();
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.playlists = Arc::new(list);
                this.covers.prefetch(urls, 48.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn do_login(&mut self, cx: &mut Context<Self>) {
        let raw = self.login_input.read(cx).value().trim().to_string();
        let Some(cookie) = siren_core::auth::normalize_cookie(&raw) else {
            self.login_status = "请输入 MUSIC_U 或完整 Cookie".into();
            cx.notify();
            return;
        };
        siren_core::auth::save_cookie(&cookie);
        // 共享客户端：Playback 与封面加载器持有同一份引用，登录态即时生效。
        self.client.set_cookie(Some(cookie));
        self.login_status = "登录校验中…".into();
        cx.notify();
        self.refresh_account(cx);
    }

    fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let account = siren_core::network::api::account(&client).await.ok();
            let _ = tx.send(account);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let account = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                let profile = account.and_then(|v| {
                    siren_core::network::api::account_uid(&v)
                        .map(|uid| (uid, siren_core::network::api::account_nickname(&v)))
                });
                match profile {
                    Some((uid, nick)) => {
                        this.logged_in = true;
                        this.uid = Some(uid);
                        this.nickname = nick.unwrap_or_else(|| "已登录".to_string()).into();
                        this.login_status = format!("已登录：{}", this.nickname).into();
                        eprintln!("[siren] 已登录：{} (uid={uid})", this.nickname);
                        this.load_my_playlists(cx);
                    }
                    None => {
                        this.logged_in = false;
                        this.login_status = "未登录（Cookie 无效或已过期）".into();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_my_playlists(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.uid else {
            return;
        };
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let list = siren_core::network::api::user_playlists(&client, uid, 50, 0)
                .await
                .unwrap_or_default();
            let _ = tx.send(list);
        });
        self.login_status = "加载我的歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.login_status = format!("我的歌单 {} 个", list.len()).into();
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.my_playlists = Arc::new(list);
                this.covers.prefetch(urls, 48.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Playlist { id, name });
        self.playlist = None;
        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let detail = siren_core::network::api::playlist_detail(&client, id)
                .await
                .ok()
                .flatten();
            let _ = tx.send(detail);
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let detail = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.playlist = detail.map(Arc::new);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_search(&mut self, cx: &mut Context<Self>) {
        let keyword = self.search_input.read(cx).value().trim().to_string();
        if keyword.is_empty() {
            return;
        }
        self.search_status = format!("搜索中：{keyword}").into();
        cx.notify();

        let client = self.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime.handle().spawn(async move {
            let songs = siren_core::network::api::search(&client, &keyword, 1, 50)
                .await
                .unwrap_or_default();
            let _ = tx.send(songs);
        });

        cx.spawn(async move |this, cx| {
            let songs = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                this.search_status = format!("搜索到 {} 首", songs.len()).into();
                this.songs = Arc::new(songs);
                this.selected = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---------- 渲染 ----------

    fn page(&self, weak: WeakEntity<Self>) -> impl IntoElement {
        match self.route() {
            Route::Home => self.home_page(weak).into_any_element(),
            Route::Library => self.library_page(weak).into_any_element(),
            Route::Search => self.search_page(weak).into_any_element(),
            Route::Playlist { name, .. } => self.playlist_page(name, weak).into_any_element(),
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let weak = cx.weak_entity();
        div()
            .v_flex()
            .size_full()
            .bg(theme::window_bg())
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .overflow_hidden()
                    .child(self.sidebar(weak.clone(), cx))
                    .child(self.page(weak.clone())),
            )
            .child(self.player_bar(weak, vw, cx))
    }
}

// ---------- 共享小部件 ----------

fn page_shell(title: String, body: AnyElement) -> impl IntoElement {
    page_shell_with_header(
        div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(title)
            .into_any_element(),
        body,
    )
}

fn page_shell_with_header(header: AnyElement, body: AnyElement) -> impl IntoElement {
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .bg(theme::pane())
        .child(div().w_full().p(px(12.0)).child(header))
        .child(body)
}

fn fmt_time(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn icon(name: IconName, size: f32, color: Rgba) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

fn cover_placeholder(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .bg(theme::cover_placeholder())
        .child(icon(IconName::Music, size * 0.36, theme::text_gray()))
}
