//! GPUI 界面。
//!
//! 页面各自为独立 `Entity`：首页 / 音乐库 / 搜索 / 歌单详情；`Root` 只做导航与外壳编排，
//! 播放逻辑集中在共享的 [`Playback`]，后端能力经 [`Services`] 下发给页面。
//!
//! 页面通过 [`PageEvent`] 事件请求导航（`OpenPlaylist` / `Back`），不依赖外壳类型。

pub mod components;
pub mod covers;
pub mod login_dialog;
mod motion;
mod notifications;
pub mod pages;
mod playback;
mod services;
mod session;
mod settings_store;
mod smooth_scroll;

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
use notifications::Notifications;
use pages::{
    AccountPage, AlbumPage, ArtistPage, HomePage, LibraryPage, PlaylistPage, ProfilePage, RecentPage, SearchPage,
};
pub(crate) use playback::flush_playback_session;
use playback::Playback;
use services::Services;
use session::Session;
use settings_store::SettingsStore;

/// 当前页面列实际宽度（供页面内自适应排布）。
pub(crate) struct PageLayout {
    pub page_w: f32,
}

impl Global for PageLayout {}

/// 左侧「音乐库」栏宽度。
pub(crate) const LIBRARY_W: f32 = 300.0;
/// 主内容行左右内边距（px 8×2）与列间距（gap 8×2）之和。
pub(crate) const SHELL_CHROME: f32 = 32.0;
/// 页面列水平内边距（首页列 `p(24)` 的两侧）。
pub(crate) const PAGE_PAD: f32 = 48.0;
/// 首页卡片定宽与间距。
pub(crate) const CARD_W: f32 = 168.0;
pub(crate) const CARD_GAP: f32 = 16.0;

/// 最小窗口尺寸：宽度需容纳「左栏 + 中间两个卡片 + 右栏」，
/// 高度取宽度的 0.618（黄金比）。
pub(crate) fn min_window_size(np_width: f32) -> (f32, f32) {
    // 额外 8px 余量：DPI 取整后可视宽度会比请求值少不到 1px，否则第二张卡
    // 会差之毫厘被挤到下一行，导致最小尺寸下只显示一张卡片。
    let w = LIBRARY_W + SHELL_CHROME + PAGE_PAD + CARD_W * 2.0 + CARD_GAP + np_width + 8.0;
    (w, w * 0.618)
}

/// 关闭主窗口时是否最小化到托盘（供引导期 `on_window_should_close` 使用；无 store 时按否）。
pub(crate) fn close_to_tray(cx: &App) -> bool {
    cx.try_global::<settings_store::SettingsStore>()
        .map(|store| store.close_to_tray())
        .unwrap_or(false)
}

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
    /// 歌手详情。
    Artist {
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
    /// 账号设置。
    Account,
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
    /// 打开歌手详情。
    OpenArtist {
        id: i64,
        name: String,
    },
    /// 打开个人主页。
    OpenProfile {
        uid: i64,
    },
    /// 打开最近播放。
    OpenRecentPlay,
    /// 打开设置。
    OpenSettings,
    /// 打开账号设置。
    OpenAccount,
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

/// 导航栈：顶层路由历史（浏览器式 history + forward 双栈，对齐 Melodia `BackStack`）。
pub(crate) struct NavState {
    stack: Vec<Route>,
    forward: Vec<Route>,
}

impl NavState {
    fn new(route: Route) -> Self {
        Self { stack: vec![route], forward: Vec::new() }
    }

    /// 当前页面。
    fn route(&self) -> Route {
        self.stack.last().cloned().unwrap_or(Route::Home)
    }

    /// 切换顶层页面（侧栏导航）：历史与前进栈一并清空。
    fn reset(&mut self, route: Route) {
        self.stack.clear();
        self.stack.push(route);
        self.forward.clear();
    }

    /// 打开详情页（push）：与当前路由相同则跳过；新导航清空前进栈。
    fn push(&mut self, route: Route) {
        if route == self.route() {
            return;
        }
        self.stack.push(route);
        self.forward.clear();
    }

    /// 返回上一页；返回的项移入前进栈。栈顶无可回退项时返回 `false`。
    fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            if let Some(route) = self.stack.pop() {
                self.forward.push(route);
            }
            true
        } else {
            false
        }
    }

    /// 前进到下一页（从前进栈搬回历史栈）。
    fn forward(&mut self) -> bool {
        match self.forward.pop() {
            Some(route) => {
                self.stack.push(route);
                true
            }
            None => false,
        }
    }

    fn can_forward(&self) -> bool {
        !self.forward.is_empty()
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
    /// 账号设置：新昵称 / 个性签名输入。
    account_nickname_input: Entity<InputState>,
    account_signature_input: Entity<InputState>,
    nav: NavState,
    home: Entity<HomePage>,
    library: Entity<LibraryPage>,
    search: Entity<SearchPage>,
    recent: Entity<RecentPage>,
    playlist_page: Option<Entity<PlaylistPage>>,
    album_page: Option<Entity<AlbumPage>>,
    artist_page: Option<Entity<ArtistPage>>,
    profile_page: Option<Entity<ProfilePage>>,
    account_page: Option<Entity<AccountPage>>,
    /// 正在播放卡片编辑器的拖拽源索引。
    card_drag_from: Option<usize>,
    /// 播放队列浮层是否打开。
    queue_open: bool,
    /// 播放队列拖动排序的源下标。
    queue_drag_from: Option<usize>,
    /// 拖动重排的行位移补间：键 → （起始像素偏移, 代次）。
    /// 渲染时把刚换过槽位的行从旧位置平滑滑到新位置（FLIP，对齐 Melodia `animateItem`）。
    drag_flip: std::collections::HashMap<String, (f32, u32)>,
    /// 播放队列滚动句柄（拖动排序换算目标下标时需补偿滚动偏移）。
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
        cx.set_global(PageLayout { page_w: 0.0 });
        let cookie = std::env::var("SIREN_COOKIE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| siren_core::auth::normalize_cookie(&s))
            .or_else(siren_core::auth::load_cookie);
        let client = Arc::new(NeteaseClient::with_cookie(cookie));
        let covers = CoverLoader::new(client.clone(), runtime.handle().clone());
        let notifications = Notifications::new(cx);
        let services = Services {
            client,
            handle: runtime.handle().clone(),
            covers,
            notifications: notifications.clone(),
        };
        let playback = Playback::new(
            window,
            cx,
            services.client.clone(),
            services.handle.clone(),
            services.covers.clone(),
            notifications.clone(),
        );

        let home = cx.new(|cx| HomePage::new(cx, services.clone(), playback.clone()));
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
        let account_nickname_input = cx.new(|cx| InputState::new(window, cx).placeholder("新昵称"));
        let account_signature_input = cx.new(|cx| InputState::new(window, cx).placeholder("个性签名"));
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
            Some("account") => Route::Account,
            Some("recent") => Route::RecentPlay,
            _ => Route::Home,
        };

        let root = cx.new(|cx| {
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
            // 提示变化时重绘外壳（Toast 浮层）
            subs.push(cx.observe(&notifications, |_root, _n, cx| cx.notify()));
            Self {
                _runtime: runtime,
                services,
                playback,
                search_input,
                pl_filter_input,
                pl_add_input,
                account_nickname_input,
                account_signature_input,
                nav: NavState::new(initial_route),
                home,
                library,
                search,
                recent,
                playlist_page: None,
                album_page: None,
                artist_page: None,
                profile_page: None,
                account_page: None,
                card_drag_from: None,
                queue_open: false,
                queue_drag_from: None,
                drag_flip: std::collections::HashMap::new(),
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
        });
        if std::env::var("SIREN_START").ok().as_deref() == Some("artist") {
            root.update(cx, |root, cx| root.open_artist(6452, "周杰伦".to_string(), cx));
        }
        if std::env::var("SIREN_START").ok().as_deref() == Some("account") {
            root.update(cx, |root, cx| root.open_account(cx));
        }
        root
    }

    fn route(&self) -> Route {
        self.nav.route()
    }

    /// 切换顶层页面（侧栏导航）。
    fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.nav.reset(route);
        self.clear_detail_pages();
        cx.notify();
    }

    fn on_page_event(&mut self, event: &PageEvent, cx: &mut Context<Self>) {
        match event {
            PageEvent::OpenPlaylist { id, name } => self.open_playlist(*id, name.clone(), cx),
            PageEvent::OpenAlbum { id, name } => self.open_album(*id, name.clone(), cx),
            PageEvent::OpenArtist { id, name } => self.open_artist(*id, name.clone(), cx),
            PageEvent::Back => self.back(cx),
            PageEvent::SignedIn => self.on_signed_in(cx),
            PageEvent::LoggedOut => self.on_signed_out(cx),
            PageEvent::OpenLogin => self.open_login(cx),
            PageEvent::OpenProfile { uid } => self.open_profile(*uid, cx),
            PageEvent::OpenRecentPlay => {
                self.recent.update(cx, |page, cx| page.load(cx));
                self.navigate(Route::RecentPlay, cx);
            }
            PageEvent::OpenSettings => self.navigate(Route::Settings, cx),
            PageEvent::OpenAccount => self.open_account(cx),
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
        let store = SettingsStore::of(cx);
        store
            .card_order()
            .iter()
            .filter(|c| !store.is_card_hidden(c))
            .filter_map(|c| NpCard::from_code(c))
            .collect()
    }

    /// 正在播放卡片编辑器布局：完整顺序 + 各项可见性。
    pub(crate) fn editor_layout(&self, cx: &App) -> Vec<(NpCard, bool)> {
        let store = SettingsStore::of(cx);
        store
            .card_order()
            .iter()
            .filter_map(|c| NpCard::from_code(c).map(|card| (card, !store.is_card_hidden(c))))
            .collect()
    }

    /// 拖拽中的源索引（供 live 重排使用）。
    pub(crate) fn card_drag_from(&self) -> Option<usize> {
        self.card_drag_from
    }

    /// 记录 / 清除拖拽源索引。
    pub(crate) fn set_card_drag_from(&mut self, idx: Option<usize>) {
        self.card_drag_from = idx;
    }

    /// 队列拖动源下标。
    pub(crate) fn queue_drag_from(&self) -> Option<usize> {
        self.queue_drag_from
    }

    /// 记录 / 清除队列拖动源下标。
    pub(crate) fn set_queue_drag_from(&mut self, idx: Option<usize>) {
        self.queue_drag_from = idx;
    }

    /// 拖动开始前清空上一次的位移补间。
    pub(crate) fn clear_drag_flip(&mut self) {
        self.drag_flip.clear();
    }

    /// 查询某行当前的归位补间（起始偏移 px，代次）。`None` 表示无需补间。
    pub(crate) fn flip_of(&self, key: &str) -> Option<(f32, u32)> {
        self.drag_flip.get(key).copied()
    }

    /// 一次重排后，为每个换了槽位的键写入起始偏移（屏幕内滑动归位）。
    /// `skip` 传入被拖动的键：它由随光标的浮动预览代表，不再自列表内滑动。
    pub(crate) fn flip_on_reorder(
        &mut self,
        old_keys: &[String],
        new_keys: &[String],
        stride: f32,
        skip: Option<&str>,
    ) {
        let slots: std::collections::HashMap<&str, usize> =
            old_keys.iter().enumerate().map(|(i, k)| (k.as_str(), i)).collect();
        for (i, key) in new_keys.iter().enumerate() {
            if skip == Some(key.as_str()) {
                continue;
            }
            let Some(&old) = slots.get(key.as_str()) else {
                continue;
            };
            if old == i {
                continue;
            }
            let generation = self.drag_flip.get(key).map_or(1, |(_, g)| g + 1);
            self.drag_flip
                .insert(key.clone(), ((old as f32 - i as f32) * stride, generation));
        }
    }

    /// 设置关闭主窗口时最小化到托盘（落盘）。
    pub(crate) fn set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        cx.global_mut::<SettingsStore>().set_close_to_tray(enabled);
        cx.notify();
    }

    /// 设置歌曲详情页是否显示滚动条（落盘）。
    pub(crate) fn set_np_scrollbar(&mut self, enabled: bool, cx: &mut Context<Self>) {
        cx.global_mut::<SettingsStore>().set_np_scrollbar(enabled);
        cx.notify();
    }

    /// 设置某卡片显隐（落盘）。
    pub(crate) fn set_card_visible(&mut self, card: NpCard, visible: bool, cx: &mut Context<Self>) {
        let store = cx.global_mut::<SettingsStore>();
        let mut hidden = store.hidden_cards();
        hidden.retain(|c| c != card.code());
        if !visible {
            hidden.push(card.code().to_string());
        }
        store.set_hidden_cards(hidden);
        cx.notify();
    }

    /// 编辑器内将 `from` 位置卡片移动到 `to`（落盘完整顺序）。
    pub(crate) fn reorder_card(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let mut order = cx.global::<SettingsStore>().card_order();
        if from < order.len() && to < order.len() && from != to {
            let card = order.remove(from);
            order.insert(to, card);
            cx.global_mut::<SettingsStore>().set_card_order(order);
            cx.notify();
        }
    }

    /// 登录成功：预取头像、加载喜欢列表与最近播放（账号真源是 [`Session`]）。
    fn on_signed_in(&mut self, cx: &mut Context<Self>) {
        if let Some(account) = Session::account(cx).cloned() {
            self.services.covers.prefetch([account.avatar_url.clone()], 32.0, cx);
            let uid = account.uid;
            self.playback.update(cx, |pb, cx| pb.load_likes(uid, cx));
            self.recent.update(cx, |page, cx| page.load(cx));
            // 调试：SIREN_START=profile 登录后直达本人主页
            if std::env::var("SIREN_START").ok().as_deref() == Some("profile") {
                self.open_profile(uid, cx);
            }
        }
        cx.notify();
    }

    /// 退出登录：清除喜欢列表。
    fn on_signed_out(&mut self, cx: &mut Context<Self>) {
        self.playback.update(cx, |pb, cx| pb.clear_likes(cx));
        cx.notify();
    }

    pub(crate) fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
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

    pub(crate) fn open_album(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Album { id, name: name.clone() });
        let page = cx.new(|cx| AlbumPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.album_page = Some(page);
        cx.notify();
    }

    fn open_artist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Artist { id, name: name.clone() });
        let page = cx.new(|cx| ArtistPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.artist_page = Some(page);
        cx.notify();
    }

    fn open_profile(&mut self, uid: i64, cx: &mut Context<Self>) {
        self.nav.push(Route::Profile { uid });
        let my_uid = self.account_uid(cx);
        let page = cx.new(|cx| ProfilePage::new(cx, self.services.clone(), uid, my_uid));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.profile_page = Some(page);
        cx.notify();
    }

    fn account_uid(&self, cx: &App) -> Option<i64> {
        Session::uid(cx)
    }

    fn open_account(&mut self, cx: &mut Context<Self>) {
        self.nav.push(Route::Account);
        let nickname_input = self.account_nickname_input.clone();
        let signature_input = self.account_signature_input.clone();
        let page = cx.new(|cx| AccountPage::new(cx, self.services.clone(), nickname_input, signature_input));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self._subs.push(sub);
        self.account_page = Some(page);
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.pop() {
            self.clear_detail_pages();
            cx.notify();
        }
    }

    /// 前进（标题栏前进按钮，浏览器语义）。
    fn forward(&mut self, cx: &mut Context<Self>) {
        if self.nav.forward() {
            self.clear_detail_pages();
            cx.notify();
        }
    }

    fn can_forward(&self) -> bool {
        self.nav.can_forward()
    }

    fn clear_detail_pages(&mut self) {
        self.playlist_page = None;
        self.album_page = None;
        self.artist_page = None;
        self.profile_page = None;
        self.account_page = None;
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

    fn page(&self, weak: WeakEntity<Self>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            Route::Artist { .. } => self
                .artist_page
                .as_ref()
                .map(|p| p.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Route::Settings => self.settings_page(weak, window, cx),
            Route::Account => self
                .account_page
                .as_ref()
                .map(|p| p.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            Route::RecentPlay => self.recent.clone().into_any_element(),
            Route::Profile { .. } => self
                .profile_page
                .as_ref()
                .map(|p| p.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
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
        let comments_overlay = self
            .comments
            .open
            .then(|| self.comments_overlay(weak_comments, window, cx));
        let weak_create = cx.weak_entity();
        let weak_picker = cx.weak_entity();
        let create_overlay = self.ops.create_open.then(|| self.create_overlay(weak_create));
        let picker_overlay = self
            .ops
            .picker_song
            .map(|_| self.picker_overlay(weak_picker, window, cx));
        let weak_rename = cx.weak_entity();
        let weak_delete = cx.weak_entity();
        let rename_overlay = self.ops.rename_pid.map(|_| self.rename_overlay(weak_rename));
        let delete_overlay = self.ops.delete_pid.map(|_| self.delete_overlay(weak_delete));
        let weak_desc = cx.weak_entity();
        let desc_overlay = self.ops.desc_pid.map(|_| self.desc_overlay(weak_desc));
        let weak_queue = cx.weak_entity();
        let queue_overlay = self.queue_open.then(|| self.queue_overlay(weak_queue, window, cx));
        // 中间列显式分配宽度，规避 gpui `flex_1` 的横向过度伸展。
        // 右侧栏严格取设置的宽度；空间不足时中间列收缩（不产生横向溢出）。
        let np_w = if self.np.open {
            SettingsStore::of(cx).np_width()
        } else {
            44.0
        };
        let page_w = (vw - LIBRARY_W - np_w - SHELL_CHROME).max(0.0);
        cx.global_mut::<PageLayout>().page_w = page_w;
        div()
            .v_flex()
            .relative()
            .size_full()
            .font_family("MiSans")
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
                            .w(px(LIBRARY_W))
                            .min_w(px(LIBRARY_W))
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
                            .child(self.page(weak_page, window, cx)),
                    )
                    .child(self.now_playing(weak.clone(), window, cx)),
            )
            .child(self.player_bar(weak, vw, cx))
            .when_some(login_overlay, |this, overlay| this.child(overlay))
            .when_some(comments_overlay, |this, overlay| this.child(overlay))
            .when_some(create_overlay, |this, overlay| this.child(overlay))
            .when_some(picker_overlay, |this, overlay| this.child(overlay))
            .when_some(rename_overlay, |this, overlay| this.child(overlay))
            .when_some(delete_overlay, |this, overlay| this.child(overlay))
            .when_some(desc_overlay, |this, overlay| this.child(overlay))
            .when_some(queue_overlay, |this, overlay| this.child(overlay))
            .child(self.notifications_overlay(cx))
    }
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

/// 毫秒时间戳 → `M月D日`。
pub(crate) fn fmt_month_day(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let (_, m, d) = civil_from_days(ms / 86_400_000);
    format!("{m}月{d}日")
}

/// 毫秒时间戳 → 分区块标题（`8月24日 周一`；跨年补年份）。无时间返回空串。
pub(crate) fn fmt_day_label(ms: i64, now_ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    const WEEKDAYS: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let days = ms / 86_400_000;
    let (y, m, d) = civil_from_days(days);
    let (ny, _, _) = civil_from_days(now_ms / 86_400_000);
    let weekday = WEEKDAYS[(days + 4).rem_euclid(7) as usize];
    let date = if y == ny {
        format!("{m}月{d}日")
    } else {
        format!("{y}年{m}月{d}日")
    };
    format!("{date} {weekday}")
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

#[cfg(test)]
mod nav_tests {
    use super::{NavState, Route};

    fn pl(id: i64) -> Route {
        Route::Playlist { id, name: format!("pl-{id}") }
    }

    #[test]
    fn push_moves_forward_and_dedups_current() {
        let mut nav = NavState::new(Route::Home);
        nav.push(pl(1));
        assert!(nav.pop());
        assert!(nav.can_forward());

        // push 新路由清空前进栈
        nav.push(Route::Search);
        assert!(!nav.can_forward());

        // 与当前路由相同则跳过（栈深不变）
        let mut nav = NavState::new(Route::Home);
        nav.push(pl(1));
        let before = nav.route();
        nav.push(pl(1));
        assert_eq!(nav.route(), before);
        assert!(nav.pop());
    }

    #[test]
    fn pop_fills_forward_and_forward_restores() {
        let mut nav = NavState::new(Route::Home);
        nav.push(pl(1));
        nav.push(pl(2));
        assert_eq!(nav.route(), pl(2));

        assert!(nav.pop());
        assert_eq!(nav.route(), pl(1));
        assert!(nav.can_forward());

        assert!(nav.forward());
        assert_eq!(nav.route(), pl(2));
        assert!(!nav.can_forward());

        // 栈底不可 pop；前进栈仍有内容时可继续 forward
        assert!(nav.pop());
        assert!(nav.pop());
        assert!(!nav.pop());
        assert!(nav.forward());
        assert_eq!(nav.route(), pl(1));
        assert!(nav.forward());
        assert_eq!(nav.route(), pl(2));
        assert!(!nav.forward());
    }

    #[test]
    fn reset_clears_history_and_forward() {
        let mut nav = NavState::new(Route::Home);
        nav.push(pl(1));
        nav.push(pl(2));
        nav.pop();
        assert!(nav.can_forward());

        nav.reset(Route::Settings);
        assert_eq!(nav.route(), Route::Settings);
        assert!(!nav.can_forward());
        assert!(!nav.pop());
    }

    #[test]
    fn back_forward_chain_across_multiple_entries() {
        let mut nav = NavState::new(Route::Home);
        nav.push(pl(1));
        nav.push(pl(2));
        nav.push(pl(3));

        assert!(nav.pop());
        assert!(nav.pop());
        assert_eq!(nav.route(), pl(1));
        // 后进先出：先 forward 回 pl(2)，再回 pl(3)
        assert!(nav.forward());
        assert_eq!(nav.route(), pl(2));
        assert!(nav.forward());
        assert_eq!(nav.route(), pl(3));
        assert!(!nav.forward());
    }
}
