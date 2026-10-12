//! GPUI 界面。
//!
//! 页面各自为独立 `Entity`：首页 / 音乐库 / 搜索 / 歌单详情；`Root` 只做导航与外壳编排，
//! 播放逻辑集中在共享的 [`Playback`]，后端能力经 [`Services`] 下发给页面。
//!
//! 页面通过 [`PageEvent`] 事件请求导航（`OpenPlaylist` / `Back`），不依赖外壳类型。

pub mod components;
pub mod covers;
pub mod login_dialog;
pub mod pages;
mod playback;
mod services;

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::network::client::NeteaseClient;

use crate::theme;
use covers::CoverLoader;
use pages::{HomePage, LibraryPage, PlaylistPage, SearchPage};
use playback::Playback;
use services::Services;

/// 当前页面（音乐库为常驻侧栏，不在此列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Home,
    Search,
    Playlist {
        id: i64,
        name: String,
    },
    /// 个人主页。
    Profile {
        uid: i64,
    },
    /// 最近播放。
    RecentPlay,
    /// 设置。
    Settings,
}

/// 页面回传给外壳的导航事件。
pub(crate) enum PageEvent {
    OpenPlaylist {
        id: i64,
        name: String,
    },
    Back,
    /// 登录成功：上报 uid、昵称与头像，供标题栏账号区展示。
    Account {
        uid: i64,
        nickname: String,
        avatar_url: String,
    },
    /// 未登录 / 已退出登录。
    LoggedOut,
    /// 请求打开登录弹窗。
    OpenLogin,
    /// 打开个人主页。
    OpenProfile {
        uid: i64,
    },
    /// 打开最近播放。
    OpenRecentPlay,
    /// 打开设置。
    OpenSettings,
}

/// 主窗口根组件：导航与外壳。
pub struct Root {
    _runtime: tokio::runtime::Runtime,
    services: Services,
    playback: Entity<Playback>,
    search_input: Entity<InputState>,
    nav: Vec<Route>,
    home: Entity<HomePage>,
    library: Entity<LibraryPage>,
    search: Entity<SearchPage>,
    playlist_page: Option<Entity<PlaylistPage>>,
    now_playing_open: bool,
    /// 登录后的账号信息（昵称、头像 URL、uid）；未登录为 None。
    account: Option<(SharedString, String, i64)>,
    /// 已为哪首歌预取过播放封面（避免重复预取）。
    cover_song: Option<i64>,
    // ── 登录弹窗状态 ───────────────────────────────────────────
    login_open: bool,
    login_tab: login_dialog::LoginTab,
    /// 二维码轮询的“代”计数：自增即可使旧任务失效。
    login_gen: u64,
    qr_image: Option<Arc<Image>>,
    login_status: SharedString,
    login_input: Entity<InputState>,
    _subs: Vec<Subscription>,
    _search_sub: Subscription,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, runtime: tokio::runtime::Runtime) -> Entity<Self> {
        siren_platform::prepare_native_environment();
        let cookie = std::env::var("SIREN_COOKIE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| siren_core::auth::normalize_cookie(&s))
            .or_else(siren_core::auth::load_cookie);
        let client = Arc::new(NeteaseClient::with_cookie(cookie));
        let covers = CoverLoader::new(client.clone(), runtime.handle().clone());
        let services = Services {
            client,
            handle: runtime.handle().clone(),
            covers,
        };
        let playback = Playback::new(window, cx, services.client.clone(), services.handle.clone());

        let home = cx.new(|cx| HomePage::new(cx, services.clone()));
        let library = cx.new(|cx| LibraryPage::new(window, cx, services.clone()));
        let search = cx.new(|_cx| SearchPage::new(services.clone(), playback.clone()));

        // 标题栏搜索输入（回车驱动搜索页）
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("想播放什么？")
                .default_value("周杰伦")
        });

        // 登录弹窗 Cookie 输入（回车提交）
        let login_input = cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 MUSIC_U 或完整 Cookie"));

        let initial_route = match std::env::var("SIREN_START").ok().as_deref() {
            Some("search") => Route::Search,
            _ => Route::Home,
        };

        cx.new(|cx| {
            let mut subs = Vec::new();
            subs.push(cx.subscribe(&home, |root: &mut Root, _e, ev: &PageEvent, cx| {
                root.on_page_event(ev, cx)
            }));
            subs.push(cx.subscribe(&library, |root: &mut Root, _e, ev: &PageEvent, cx| {
                root.on_page_event(ev, cx)
            }));
            let _search_sub = cx.subscribe(&search_input, |root: &mut Root, _i, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    root.submit_search(cx);
                }
            });
            let login_sub = cx.subscribe(&login_input, |root: &mut Root, _i, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    root.submit_cookie_login(cx);
                }
            });
            subs.push(login_sub);
            // 观察播放状态：切歌时预取封面，并驱动外壳（播放条/正在播放栏）重绘
            subs.push(cx.observe(&playback, |root: &mut Root, playback, cx| {
                root.on_playback(playback, cx)
            }));
            Self {
                _runtime: runtime,
                services,
                playback,
                search_input,
                nav: vec![initial_route],
                home,
                library,
                search,
                playlist_page: None,
                now_playing_open: true,
                account: None,
                cover_song: None,
                login_open: false,
                login_tab: login_dialog::LoginTab::Scan,
                login_gen: 0,
                qr_image: None,
                login_status: SharedString::default(),
                login_input,
                _subs: subs,
                _search_sub,
            }
        })
    }

    fn route(&self) -> Route {
        self.nav.last().cloned().unwrap_or(Route::Home)
    }

    /// 切换顶层页面（侧栏导航）。
    fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.nav.clear();
        self.nav.push(route);
        self.playlist_page = None;
        cx.notify();
    }

    fn on_page_event(&mut self, event: &PageEvent, cx: &mut Context<Self>) {
        match event {
            PageEvent::OpenPlaylist { id, name } => self.open_playlist(*id, name.clone(), cx),
            PageEvent::Back => self.back(cx),
            PageEvent::Account {
                uid,
                nickname,
                avatar_url,
            } => self.set_account(Some((*uid, nickname.clone(), avatar_url.clone())), cx),
            PageEvent::LoggedOut => self.set_account(None, cx),
            PageEvent::OpenLogin => self.open_login(cx),
            PageEvent::OpenProfile { uid } => self.navigate(Route::Profile { uid: *uid }, cx),
            PageEvent::OpenRecentPlay => self.navigate(Route::RecentPlay, cx),
            PageEvent::OpenSettings => self.navigate(Route::Settings, cx),
        }
    }

    /// 播放状态变化：切歌时预取其封面（播放条 56 / 正在播放栏 220），并重绘外壳。
    fn on_playback(&mut self, playback: Entity<Playback>, cx: &mut Context<Self>) {
        let song = playback.read(cx).state().song;
        let id = song.as_ref().map(|s| s.id);
        if id != self.cover_song {
            self.cover_song = id;
            if let Some(song) = &song {
                self.services.covers.prefetch([song.cover_url.clone()], 56.0, cx);
                self.services.covers.prefetch([song.cover_url.clone()], 220.0, cx);
            }
        }
        cx.notify();
    }

    /// 更新标题栏账号区（登录成功 / 退出登录）。
    fn set_account(&mut self, account: Option<(i64, String, String)>, cx: &mut Context<Self>) {
        match &account {
            Some((uid, _, url)) => {
                self.services.covers.prefetch([url.clone()], 32.0, cx);
                let uid = *uid;
                self.playback.update(cx, |pb, cx| pb.load_likes(uid, cx));
            }
            None => self.playback.update(cx, |pb, cx| pb.clear_likes(cx)),
        }
        self.account = account.map(|(uid, nick, url)| (nick.into(), url, uid));
        cx.notify();
    }

    fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Playlist { id, name: name.clone() });
        let page = cx.new(|cx| PlaylistPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.playlist_page = Some(page);
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.len() > 1 {
            self.nav.pop();
            self.playlist_page = None;
            cx.notify();
        }
    }

    /// 退出登录（标题栏头像菜单）：清除 Cookie 并让音乐库回到未登录态。
    fn logout(&mut self, cx: &mut Context<Self>) {
        self.library.update(cx, |page, cx| page.logout(cx));
        cx.notify();
    }

    /// 收起 / 展开正在播放栏。
    fn toggle_now_playing(&mut self, cx: &mut Context<Self>) {
        self.now_playing_open = !self.now_playing_open;
        cx.notify();
    }

    /// 标题栏搜索：回车 → 切到搜索页并以其关键字检索。
    fn submit_search(&mut self, cx: &mut Context<Self>) {
        let keyword = self.search_input.read(cx).value().trim().to_string();
        if keyword.is_empty() {
            return;
        }
        if !matches!(self.route(), Route::Search) {
            self.navigate(Route::Search, cx);
        }
        self.search.update(cx, |page, cx| page.search(keyword, cx));
    }

    fn page(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        match self.route() {
            Route::Home => self.home.clone().into_any_element(),
            Route::Search => self.search.clone().into_any_element(),
            Route::Playlist { .. } => self
                .playlist_page
                .as_ref()
                .map(|p| p.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Route::Settings => self.settings_page(weak, cx),
            // 以下页面先接路由占位，后续逐个补齐
            Route::Profile { .. } => placeholder_page("个人主页", "个人主页（待实现）"),
            Route::RecentPlay => placeholder_page("最近播放", "最近播放（待实现）"),
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let weak = cx.weak_entity();
        let weak_page = cx.weak_entity();
        let weak_login = cx.weak_entity();
        let login_overlay = self.login_open.then(|| self.login_overlay(weak_login));
        // 中间列显式分配宽度，规避 gpui `flex_1` 的横向过度伸展
        let np_w = if self.now_playing_open { 340.0 } else { 44.0 };
        let page_w = (vw - 300.0 - np_w - 32.0).max(240.0);
        div()
            .v_flex()
            .size_full()
            .bg(theme::window_bg())
            .child(self.title_bar(weak.clone(), vw, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(300.0))
                            .min_w(px(300.0))
                            .h_full()
                            .flex_none()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child(self.library.clone()),
                    )
                    .child(
                        div()
                            .w(px(page_w))
                            .flex_none()
                            .h_full()
                            .overflow_hidden()
                            .child(self.page(weak_page, cx)),
                    )
                    .child(self.now_playing(weak.clone(), cx)),
            )
            .child(self.player_bar(weak, vw, cx))
            .when_some(login_overlay, |this, overlay| this.child(overlay))
    }
}

/// 占位页面（路由已接，内容待补）。
fn placeholder_page(title: &str, hint: &str) -> AnyElement {
    page_shell(
        title.to_string(),
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(div().text_sm().text_color(theme::text_gray()).child(hint.to_string()))
            .into_any_element(),
    )
    .into_any_element()
}

// ---------- 共享小部件 ----------

pub(crate) fn page_shell(title: String, body: AnyElement) -> impl IntoElement {
    page_shell_with_header(
        div()
            .text_lg()
            .text_color(theme::text_primary())
            .child(title)
            .into_any_element(),
        body,
    )
}

pub(crate) fn page_shell_with_header(header: AnyElement, body: AnyElement) -> impl IntoElement {
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .bg(theme::pane())
        .child(div().w_full().p(px(12.0)).child(header))
        .child(body)
}

pub(crate) fn fmt_time(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub(crate) fn icon(name: IconName, size: f32, color: Rgba) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

pub(crate) fn cover_placeholder(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .bg(theme::cover_placeholder())
        .child(icon(IconName::Music, size * 0.5, theme::text_gray()))
}

/// 圆形占位（头像）。返回 `AnyElement` 以便与 [`crate::ui::covers`] 的图片互替。
pub(crate) fn cover_placeholder_round(size: f32) -> AnyElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme::cover_placeholder())
        .child(icon(IconName::Music, size * 0.5, theme::text_gray()))
        .into_any_element()
}
