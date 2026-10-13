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
mod session;
mod settings_store;

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::Playlist;
use siren_core::network::client::NeteaseClient;

use crate::theme;
use covers::CoverLoader;
use pages::{AlbumPage, HomePage, LibraryPage, PlaylistPage, RecentPage, SearchPage};
use playback::Playback;
use services::Services;
use session::Session;
use settings_store::SettingsStore;

/// 当前页面（音乐库为常驻侧栏，不在此列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Route {
    Home,
    Search,
    Playlist {
        id: i64,
        name: String,
    },
    /// 专辑详情。
    Album {
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
    /// 打开专辑详情。
    OpenAlbum {
        id: i64,
        name: String,
    },
    Back,
    /// 登录成功（账号信息经 [`Session`] 全局读取）。
    SignedIn,
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
    /// 请求新建歌单。
    OpenCreate,
    /// 请求把歌曲加入歌单。
    AddToPlaylist {
        song_id: i64,
    },
    /// 请求重命名歌单。
    RenamePlaylist {
        id: i64,
    },
    /// 请求删除歌单。
    DeletePlaylist {
        id: i64,
    },
    /// 请求编辑歌单简介。
    EditPlaylistDesc {
        id: i64,
    },
    /// 歌单信息已变更（封面等），请外壳刷新。
    PlaylistChanged,
}

/// 正在播放栏的卡片（可配置顺序/显隐）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NpCard {
    Lyrics,
    Comments,
    SongDetail,
    MusicMemory,
    AboutArtist,
    ArtistAlbums,
    SimilarArtists,
}

impl NpCard {
    pub(crate) const ALL: [NpCard; 7] = [
        NpCard::Lyrics,
        NpCard::Comments,
        NpCard::SongDetail,
        NpCard::MusicMemory,
        NpCard::AboutArtist,
        NpCard::ArtistAlbums,
        NpCard::SimilarArtists,
    ];

    pub(crate) fn code(self) -> &'static str {
        match self {
            NpCard::Lyrics => "lyrics",
            NpCard::Comments => "comments",
            NpCard::SongDetail => "song_detail",
            NpCard::MusicMemory => "music_memory",
            NpCard::AboutArtist => "about_artist",
            NpCard::ArtistAlbums => "artist_albums",
            NpCard::SimilarArtists => "similar_artists",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            NpCard::Lyrics => "歌词预览",
            NpCard::Comments => "评论",
            NpCard::SongDetail => "歌曲详情",
            NpCard::MusicMemory => "回忆坐标",
            NpCard::AboutArtist => "关于歌手",
            NpCard::ArtistAlbums => "歌手专辑",
            NpCard::SimilarArtists => "相似歌手",
        }
    }

    pub(crate) fn from_code(code: &str) -> Option<NpCard> {
        NpCard::ALL.into_iter().find(|c| c.code() == code)
    }
}

/// 导航栈：顶层路由历史。
pub(crate) struct NavState {
    stack: Vec<Route>,
}

impl NavState {
    fn new(route: Route) -> Self {
        Self { stack: vec![route] }
    }

    /// 当前页面。
    fn route(&self) -> Route {
        self.stack.last().cloned().unwrap_or(Route::Home)
    }

    /// 切换顶层页面：历史重置为单元素。
    fn reset(&mut self, route: Route) {
        self.stack.clear();
        self.stack.push(route);
    }

    fn push(&mut self, route: Route) {
        self.stack.push(route);
    }

    /// 返回上一页；栈顶无可回退项时返回 `false`。
    fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }

    fn last_mut(&mut self) -> Option<&mut Route> {
        self.stack.last_mut()
    }
}

/// 登录弹窗状态。
pub(crate) struct LoginDialogState {
    pub(crate) open: bool,
    pub(crate) tab: login_dialog::LoginTab,
    /// 二维码轮询的「代」计数：自增即可使旧任务失效。
    pub(crate) gen: u64,
    pub(crate) qr_image: Option<Arc<Image>>,
    pub(crate) status: SharedString,
    pub(crate) input: Entity<InputState>,
}

/// 歌曲评论面板状态。
#[derive(Default)]
pub(crate) struct CommentsPanelState {
    pub(crate) open: bool,
    pub(crate) items: Vec<siren_core::model::CommentItem>,
    pub(crate) total: i64,
    pub(crate) has_more: bool,
    pub(crate) cursor: String,
    pub(crate) floor_open: Option<i64>,
    pub(crate) floor_items: Vec<siren_core::model::CommentItem>,
}

/// 歌单写操作浮层状态。
pub(crate) struct PlaylistOpsState {
    pub(crate) create_open: bool,
    pub(crate) create_input: Entity<InputState>,
    pub(crate) picker_song: Option<i64>,
    pub(crate) picker_playlists: Vec<Playlist>,
    pub(crate) rename_pid: Option<i64>,
    pub(crate) rename_input: Entity<InputState>,
    pub(crate) delete_pid: Option<i64>,
    pub(crate) desc_pid: Option<i64>,
    pub(crate) desc_input: Entity<InputState>,
}

/// 正在播放栏的 UI 状态。
pub(crate) struct NowPlayingUi {
    pub(crate) open: bool,
    pub(crate) album_page: usize,
    pub(crate) simi_page: usize,
    /// 已为哪首歌预取过播放封面（避免重复预取）。
    pub(crate) cover_song: Option<i64>,
}

/// 主窗口根组件：导航与外壳。
pub struct Root {
    _runtime: tokio::runtime::Runtime,
    services: Services,
    playback: Entity<Playback>,
    search_input: Entity<InputState>,
    /// 歌单页内搜索框（跨歌单复用）。
    pl_filter_input: Entity<InputState>,
    /// 歌单「添加歌曲」搜索框（跨歌单复用）。
    pl_add_input: Entity<InputState>,
    nav: NavState,
    home: Entity<HomePage>,
    library: Entity<LibraryPage>,
    search: Entity<SearchPage>,
    recent: Entity<RecentPage>,
    playlist_page: Option<Entity<PlaylistPage>>,
    album_page: Option<Entity<AlbumPage>>,
    np: NowPlayingUi,
    np_width_slider: Entity<SliderState>,
    login: LoginDialogState,
    comments: CommentsPanelState,
    ops: PlaylistOpsState,
    _subs: Vec<Subscription>,
    _search_sub: Subscription,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, runtime: tokio::runtime::Runtime) -> Entity<Self> {
        siren_platform::prepare_native_environment();
        // 全局唯一真源：设置 + 会话
        SettingsStore::init(cx);
        Session::init(cx);
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
        let playback = Playback::new(
            window,
            cx,
            services.client.clone(),
            services.handle.clone(),
            services.covers.clone(),
        );

        let home = cx.new(|cx| HomePage::new(cx, services.clone()));
        let library = cx.new(|cx| LibraryPage::new(window, cx, services.clone()));
        let search = cx.new(|_cx| SearchPage::new(services.clone(), playback.clone()));
        let recent = cx.new(|_cx| RecentPage::new(services.clone(), playback.clone()));

        // 标题栏搜索输入（回车驱动搜索页）
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("想播放什么？")
                .default_value("周杰伦")
        });

        // 登录弹窗 Cookie 输入（回车提交）
        let login_input = cx.new(|cx| InputState::new(window, cx).placeholder("粘贴 MUSIC_U 或完整 Cookie"));
        // 新建歌单名称输入
        let create_input = cx.new(|cx| InputState::new(window, cx).placeholder("歌单名称"));
        let pl_filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索歌单里的歌曲"));
        let pl_add_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索要添加的歌曲"));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("新名称"));
        let desc_input = cx.new(|cx| InputState::new(window, cx).placeholder("歌单简介"));

        let np_width = SettingsStore::of(cx).np_width();
        let np_width_slider = cx.new(|_| {
            SliderState::new()
                .max(siren_core::settings::NP_WIDTH_MAX)
                .min(siren_core::settings::NP_WIDTH_MIN)
                .step(1.0)
                .default_value(np_width)
        });

        let initial_route = match std::env::var("SIREN_START").ok().as_deref() {
            Some("search") => Route::Search,
            Some("settings") => Route::Settings,
            Some("recent") => Route::RecentPlay,
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
            subs.push(cx.subscribe(&search, |root: &mut Root, _e, ev: &PageEvent, cx| {
                root.on_page_event(ev, cx)
            }));
            subs.push(cx.subscribe(&recent, |root: &mut Root, _e, ev: &PageEvent, cx| {
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
            subs.push(cx.subscribe(&create_input, |root: &mut Root, _i, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    root.submit_create(cx);
                }
            }));
            subs.push(cx.subscribe(&rename_input, |root: &mut Root, _i, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    root.submit_rename(cx);
                }
            }));
            subs.push(cx.subscribe(&desc_input, |root: &mut Root, _i, ev: &InputEvent, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    root.submit_desc(cx);
                }
            }));
            subs.push(
                cx.subscribe(&np_width_slider, |root: &mut Root, _slider, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(SliderValue::Single(f)) = event {
                        root.set_np_width(*f, cx);
                    }
                }),
            );
            // 观察播放状态：切歌时预取封面，并驱动外壳（播放条/正在播放栏）重绘
            subs.push(cx.observe(&playback, |root: &mut Root, playback, cx| {
                root.on_playback(playback, cx)
            }));
            Self {
                _runtime: runtime,
                services,
                playback,
                search_input,
                pl_filter_input,
                pl_add_input,
                nav: NavState::new(initial_route),
                home,
                library,
                search,
                recent,
                playlist_page: None,
                album_page: None,
                np: NowPlayingUi {
                    open: true,
                    album_page: 0,
                    simi_page: 0,
                    cover_song: None,
                },
                np_width_slider,
                login: LoginDialogState {
                    open: false,
                    tab: login_dialog::LoginTab::Scan,
                    gen: 0,
                    qr_image: None,
                    status: SharedString::default(),
                    input: login_input,
                },
                comments: CommentsPanelState {
                    cursor: "0".to_string(),
                    ..Default::default()
                },
                ops: PlaylistOpsState {
                    create_open: false,
                    create_input,
                    picker_song: None,
                    picker_playlists: Vec::new(),
                    rename_pid: None,
                    rename_input,
                    delete_pid: None,
                    desc_pid: None,
                    desc_input,
                },
                _subs: subs,
                _search_sub,
            }
        })
    }

    fn route(&self) -> Route {
        self.nav.route()
    }

    /// 切换顶层页面（侧栏导航）。
    fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.nav.reset(route);
        self.playlist_page = None;
        self.album_page = None;
        cx.notify();
    }

    fn on_page_event(&mut self, event: &PageEvent, cx: &mut Context<Self>) {
        match event {
            PageEvent::OpenPlaylist { id, name } => self.open_playlist(*id, name.clone(), cx),
            PageEvent::OpenAlbum { id, name } => self.open_album(*id, name.clone(), cx),
            PageEvent::Back => self.back(cx),
            PageEvent::SignedIn => self.on_signed_in(cx),
            PageEvent::LoggedOut => self.on_signed_out(cx),
            PageEvent::OpenLogin => self.open_login(cx),
            PageEvent::OpenProfile { uid } => self.navigate(Route::Profile { uid: *uid }, cx),
            PageEvent::OpenRecentPlay => {
                self.recent.update(cx, |page, cx| page.load(cx));
                self.navigate(Route::RecentPlay, cx);
            }
            PageEvent::OpenSettings => self.navigate(Route::Settings, cx),
            PageEvent::OpenCreate => self.open_create(cx),
            PageEvent::AddToPlaylist { song_id } => self.open_picker(*song_id, cx),
            PageEvent::RenamePlaylist { id } => self.open_rename(*id, cx),
            PageEvent::DeletePlaylist { id } => self.open_delete(*id, cx),
            PageEvent::EditPlaylistDesc { id } => self.open_desc(*id, cx),
            PageEvent::PlaylistChanged => self.library.update(cx, |page, cx| page.reload_playlists(cx)),
        }
    }

    /// 播放状态变化：切歌时预取其封面（播放条 56 / 正在播放栏 220），并重绘外壳。
    fn on_playback(&mut self, playback: Entity<Playback>, cx: &mut Context<Self>) {
        let song = playback.read(cx).state().song;
        let id = song.as_ref().map(|s| s.id);
        if id != self.np.cover_song {
            self.np.cover_song = id;
            if let Some(song) = &song {
                self.services.covers.prefetch([song.cover_url.clone()], 56.0, cx);
                self.services.covers.prefetch([song.cover_url.clone()], 512.0, cx);
            }
        }
        cx.notify();
    }

    /// 设置右侧栏宽度（写入设置 store 并落盘，夹在 300..800）。
    fn set_np_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let w = width.clamp(siren_core::settings::NP_WIDTH_MIN, siren_core::settings::NP_WIDTH_MAX);
        if (w - SettingsStore::of(cx).np_width()).abs() < 0.5 {
            return;
        }
        cx.global_mut::<SettingsStore>().set_np_width(w);
        cx.notify();
    }

    /// 设置右侧栏宽度（从设置页调用）。
    pub(crate) fn np_width_slider(&self) -> &Entity<SliderState> {
        &self.np_width_slider
    }

    /// 当前右侧栏宽度（唯一真源：设置 store）。
    pub(crate) fn np_width(&self, cx: &App) -> f32 {
        SettingsStore::of(cx).np_width()
    }

    /// 当前正在播放卡片顺序（唯一真源：设置 store）。
    pub(crate) fn np_cards(&self, cx: &App) -> Vec<NpCard> {
        SettingsStore::of(cx)
            .cards()
            .iter()
            .filter_map(|c| NpCard::from_code(c))
            .collect()
    }

    /// 保存正在播放卡片顺序（写入设置 store 并落盘）。
    pub(crate) fn set_np_cards(&mut self, cards: Vec<NpCard>, cx: &mut Context<Self>) {
        let codes: Vec<String> = cards.iter().map(|c| c.code().to_string()).collect();
        cx.global_mut::<SettingsStore>().set_cards(codes);
        cx.notify();
    }

    /// 登录成功：预取头像、加载喜欢列表与最近播放（账号真源是 [`Session`]）。
    fn on_signed_in(&mut self, cx: &mut Context<Self>) {
        if let Some(account) = Session::account(cx).cloned() {
            self.services.covers.prefetch([account.avatar_url.clone()], 32.0, cx);
            let uid = account.uid;
            self.playback.update(cx, |pb, cx| pb.load_likes(uid, cx));
            self.recent.update(cx, |page, cx| page.load(cx));
        }
        cx.notify();
    }

    /// 退出登录：清除喜欢列表。
    fn on_signed_out(&mut self, cx: &mut Context<Self>) {
        self.playback.update(cx, |pb, cx| pb.clear_likes(cx));
        cx.notify();
    }

    fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Playlist { id, name: name.clone() });
        let filter = self.pl_filter_input.clone();
        let add_input = self.pl_add_input.clone();
        let my_uid = Session::uid(cx);
        let page = cx.new(|cx| {
            PlaylistPage::new(
                cx,
                self.services.clone(),
                self.playback.clone(),
                id,
                name,
                filter,
                add_input,
                my_uid,
            )
        });
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.playlist_page = Some(page);
        cx.notify();
    }

    fn open_album(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Album { id, name: name.clone() });
        let page = cx.new(|cx| AlbumPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.album_page = Some(page);
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.pop() {
            self.playlist_page = None;
            self.album_page = None;
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
        self.np.open = !self.np.open;
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
            Route::Album { .. } => self
                .album_page
                .as_ref()
                .map(|p| p.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Route::Settings => self.settings_page(weak, cx),
            Route::RecentPlay => self.recent.clone().into_any_element(),
            // 以下页面先接路由占位，后续逐个补齐
            Route::Profile { .. } => placeholder_page("个人主页", "个人主页（待实现）"),
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = f32::from(window.viewport_size().width);
        let weak = cx.weak_entity();
        let weak_page = cx.weak_entity();
        let weak_login = cx.weak_entity();
        let weak_comments = cx.weak_entity();
        let login_overlay = self.login.open.then(|| self.login_overlay(weak_login));
        let comments_overlay = self.comments.open.then(|| self.comments_overlay(weak_comments));
        let weak_create = cx.weak_entity();
        let weak_picker = cx.weak_entity();
        let create_overlay = self.ops.create_open.then(|| self.create_overlay(weak_create));
        let picker_overlay = self.ops.picker_song.map(|_| self.picker_overlay(weak_picker));
        let weak_rename = cx.weak_entity();
        let weak_delete = cx.weak_entity();
        let rename_overlay = self.ops.rename_pid.map(|_| self.rename_overlay(weak_rename));
        let delete_overlay = self.ops.delete_pid.map(|_| self.delete_overlay(weak_delete));
        let weak_desc = cx.weak_entity();
        let desc_overlay = self.ops.desc_pid.map(|_| self.desc_overlay(weak_desc));
        // 中间列显式分配宽度，规避 gpui `flex_1` 的横向过度伸展。
        // 右侧栏严格取设置的宽度；空间不足时中间列收缩（不产生横向溢出）。
        let np_w = if self.np.open {
            SettingsStore::of(cx).np_width()
        } else {
            44.0
        };
        let page_w = (vw - 300.0 - np_w - 32.0).max(0.0);
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
            .when_some(comments_overlay, |this, overlay| this.child(overlay))
            .when_some(create_overlay, |this, overlay| this.child(overlay))
            .when_some(picker_overlay, |this, overlay| this.child(overlay))
            .when_some(rename_overlay, |this, overlay| this.child(overlay))
            .when_some(delete_overlay, |this, overlay| this.child(overlay))
            .when_some(desc_overlay, |this, overlay| this.child(overlay))
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

/// 毫秒时间戳 → `YYYY-MM-DD`（无日期时返回空串）。
pub(crate) fn fmt_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let (y, m, d) = civil_from_days(ms / 86_400_000);
    format!("{y}-{m:02}-{d:02}")
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)（Howard Hinnant 算法）。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
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
