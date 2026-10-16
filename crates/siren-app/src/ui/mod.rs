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
use gpui_kit::base::{transition, StyledExt, Transition};
use gpui_kit::component::button::Button;
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
/// 「我的歌单」/「加入歌单」每页条数（分页加载）。
pub(crate) const PLAYLIST_PAGE: u32 = 50;
/// 主内容行左右内边距（px 8×2）与列间距（gap 8×2）之和。
pub(crate) const SHELL_CHROME: f32 = 32.0;
/// 页面列水平内边距（首页列 `p(24)` 的两侧）。
pub(crate) const PAGE_PAD: f32 = 48.0;
/// 首页卡片定宽与间距。
pub(crate) const CARD_W: f32 = 168.0;
pub(crate) const CARD_GAP: f32 = 16.0;
/// 右栏折叠后的宽度（展开按钮竖条）。
pub(crate) const NP_COLLAPSED_W: f32 = 44.0;
/// 中间列最小内容宽（首页两卡片 + 列内边距，与 `min_window_size` 同一口径）。
pub(crate) const PAGE_CONTENT_MIN: f32 = PAGE_PAD + CARD_W * 2.0 + CARD_GAP;

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
    /// 请求删除歌单（name 供删除确认浮层展示）。
    DeletePlaylist {
        id: i64,
        name: String,
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

/// 导航栈最大深度（历史栈与前进栈各自独立）：超出时丢弃栈底最早的记录。
const NAV_MAX_DEPTH: usize = 10;

/// 导航栈：顶层路由历史（浏览器式 history + forward 双栈，对齐 Melodia `BackStack`）。
pub(crate) struct NavState {
    stack: Vec<Route>,
    forward: Vec<Route>,
}

impl NavState {
    fn new(route: Route) -> Self {
        Self {
            stack: vec![route],
            forward: Vec::new(),
        }
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
    /// 深度超过 [`NAV_MAX_DEPTH`] 时丢弃栈底最早的记录。
    fn push(&mut self, route: Route) {
        if route == self.route() {
            return;
        }
        self.stack.push(route);
        self.forward.clear();
        self.trim();
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
                self.trim();
                true
            }
            None => false,
        }
    }

    /// 深度超限时丢弃两栈栈底（最早）的记录，保留最近的历史与前进目标。
    fn trim(&mut self) {
        if self.stack.len() > NAV_MAX_DEPTH {
            let excess = self.stack.len() - NAV_MAX_DEPTH;
            self.stack.drain(0..excess);
        }
        if self.forward.len() > NAV_MAX_DEPTH {
            let excess = self.forward.len() - NAV_MAX_DEPTH;
            self.forward.drain(0..excess);
        }
    }

    fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// 栈底（仅剩一项）不可回退。
    fn can_back(&self) -> bool {
        self.stack.len() > 1
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
    /// 面板归属的歌曲 id：切歌后据此重置/校验，避免新旧评论混列表（J6）。
    pub(crate) song_id: Option<i64>,
    /// 「加载更多」是否在途：防止连点重复分页（J6）。
    pub(crate) loading_more: bool,
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
    /// 「加入歌单」列表分页：已加载条数与是否还有更多（K1）。
    pub(crate) picker_offset: u32,
    pub(crate) picker_more: bool,
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
    /// 队列封面预取的签名（长度/当前下标/当前曲 id）；变化时才预取，避免每帧发起。
    queue_cover_sig: Option<(usize, Option<usize>, Option<i64>)>,
    /// 播放队列拖动排序的源下标。
    queue_drag_from: Option<usize>,
    /// 「清空队列」确认浮层是否打开。
    queue_clear_open: bool,
    /// 标题栏自绘拖动（CSD）：按下置位，首次移动交给合成器后清除。
    tb_drag_armed: bool,
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
    /// 详情页订阅：按路由持有，替换/离开时 drop，避免只增不减（J3）。
    playlist_sub: Option<Subscription>,
    album_sub: Option<Subscription>,
    artist_sub: Option<Subscription>,
    profile_sub: Option<Subscription>,
    account_sub: Option<Subscription>,
}

impl Root {
    pub fn new(window: &mut Window, cx: &mut App, handle: tokio::runtime::Handle) -> Entity<Self> {
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
        let covers = CoverLoader::new(client.clone(), handle.clone());
        let notifications = Notifications::new(cx);
        let services = Services {
            client,
            handle: handle.clone(),
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
        // 冷启动恢复出的当前曲：立即拉取其元数据，打开详情页即有内容。
        playback.update(cx, |pb, cx| pb.load_restored_metadata(cx));

        let home = cx.new(|cx| HomePage::new(cx, services.clone(), playback.clone()));
        let library = cx.new(|cx| LibraryPage::new(window, cx, services.clone()));
        let search = cx.new(|_cx| SearchPage::new(services.clone(), playback.clone()));
        let recent = cx.new(|_cx| RecentPage::new(services.clone(), playback.clone()));

        // 标题栏搜索输入（回车驱动搜索页）
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("想播放什么？"));

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
                    match event {
                        // 拖动中只改内存，结束（Release）才落盘，避免逐像素写盘。
                        SliderEvent::Change(SliderValue::Single(f)) => root.set_np_width_live(*f, cx),
                        SliderEvent::Release(SliderValue::Single(f)) => root.set_np_width(*f, cx),
                        _ => {}
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
                queue_cover_sig: None,
                queue_drag_from: None,
                queue_clear_open: false,
                tb_drag_armed: false,
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
                    picker_offset: 0,
                    picker_more: false,
                    rename_pid: None,
                    rename_input,
                    delete_pid: None,
                    desc_pid: None,
                    desc_input,
                },
                _subs: subs,
                _search_sub,
                playlist_sub: None,
                album_sub: None,
                artist_sub: None,
                profile_sub: None,
                account_sub: None,
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
        self.ensure_route_page(cx);
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
            PageEvent::DeletePlaylist { id, name } => self.open_delete(*id, name.clone(), cx),
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
            // 切歌：评论面板归属旧歌时关闭，避免新旧歌评论混列表（J6）。
            if self.comments.open && self.comments.song_id != id {
                self.comments.open = false;
                self.comments.loading_more = false;
            }
        }
        // 队列封面：面板打开且队列结构变化时预取一次（不在渲染路径里每帧发起网络）。
        if self.queue_open {
            let st = playback.read(cx).state();
            let sig = (st.queue_len, st.current_index, st.song.as_ref().map(|s| s.id));
            if self.queue_cover_sig != Some(sig) {
                self.queue_cover_sig = Some(sig);
                let (items, _) = playback.read(cx).queue_snapshot();
                self.services
                    .covers
                    .prefetch(items.iter().map(|s| s.cover_url.clone()), 40.0, cx);
            }
        }
        // 切歌：当前页面实体补一次 notify，驱动页面内的「当前播放」高亮刷新。
        match self.route() {
            Route::Home => {
                self.home.update(cx, |_, cx| cx.notify());
            }
            Route::Search => {
                self.search.update(cx, |_, cx| cx.notify());
            }
            Route::RecentPlay => {
                self.recent.update(cx, |_, cx| cx.notify());
            }
            Route::Playlist { .. } => {
                if let Some(page) = &self.playlist_page {
                    page.update(cx, |_, cx| cx.notify());
                }
            }
            Route::Album { .. } => {
                if let Some(page) = &self.album_page {
                    page.update(cx, |_, cx| cx.notify());
                }
            }
            Route::Artist { .. } => {
                if let Some(page) = &self.artist_page {
                    page.update(cx, |_, cx| cx.notify());
                }
            }
            Route::Profile { .. } => {
                if let Some(page) = &self.profile_page {
                    page.update(cx, |_, cx| cx.notify());
                }
            }
            Route::Account => {
                if let Some(page) = &self.account_page {
                    page.update(cx, |_, cx| cx.notify());
                }
            }
            Route::Settings => {}
        }
        cx.notify();
    }

    /// 开关播放队列浮层；打开时预取一次队列封面并滚动定位到当前曲。
    pub(crate) fn set_queue_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.queue_open = open;
        if !open {
            self.queue_clear_open = false;
            self.queue_cover_sig = None;
            return;
        }
        let (items, current) = self.playback.read(cx).queue_snapshot();
        let song_id = current.and_then(|c| items.get(c)).map(|s| s.id);
        self.queue_cover_sig = Some((items.len(), current, song_id));
        self.services
            .covers
            .prefetch(items.iter().map(|s| s.cover_url.clone()), 40.0, cx);
        if !items.is_empty() {
            self.scroll_queue_to_current(cx);
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

    /// 仅更新内存宽度（拖动中，不落盘）。
    fn set_np_width_live(&mut self, width: f32, cx: &mut Context<Self>) {
        let w = width.clamp(siren_core::settings::NP_WIDTH_MIN, siren_core::settings::NP_WIDTH_MAX);
        if (w - SettingsStore::of(cx).np_width()).abs() < 0.5 {
            return;
        }
        cx.global_mut::<SettingsStore>().set_np_width_live(w);
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

    /// 右侧栏当前有效宽（N6.2 联动）：展开时受「窗口宽 − 侧栏 − 中间列最小内容宽」约束，
    /// 拖到 800 也只压缩右栏自身显示宽，不挤没中间列、不把右栏裁剪出窗口；折叠为定宽条。
    pub(crate) fn np_effective_w(&self, vw: f32, cx: &App) -> f32 {
        if !self.np.open {
            return NP_COLLAPSED_W;
        }
        let max_np = (vw - LIBRARY_W - SHELL_CHROME - PAGE_CONTENT_MIN).max(NP_COLLAPSED_W);
        self.np_width(cx).min(max_np)
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

    /// 打开「清空队列」确认浮层。
    pub(crate) fn open_queue_clear(&mut self, cx: &mut Context<Self>) {
        self.queue_clear_open = true;
        cx.notify();
    }

    /// 关闭「清空队列」确认浮层。
    pub(crate) fn close_queue_clear(&mut self, cx: &mut Context<Self>) {
        self.queue_clear_open = false;
        cx.notify();
    }

    /// 确认清空队列（保留当前曲）。
    pub(crate) fn confirm_queue_clear(&mut self, cx: &mut Context<Self>) {
        self.queue_clear_open = false;
        self.playback.update(cx, |pb, cx| pb.clear_queue(cx));
    }

    /// 历史栈是否可回退（供标题栏回退按钮置灰）。
    fn can_back(&self) -> bool {
        self.nav.can_back()
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

    /// 创建歌单详情页实体（不触碰导航栈；供 `open_*` 与 `back`/`forward` 复用）。
    fn make_playlist_page(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
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
        self.playlist_sub = Some(sub);
        self.playlist_page = Some(page);
    }

    fn make_album_page(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        let page = cx.new(|cx| AlbumPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self.album_sub = Some(sub);
        self.album_page = Some(page);
    }

    fn make_artist_page(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        let page = cx.new(|cx| ArtistPage::new(cx, self.services.clone(), self.playback.clone(), id, name));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self.artist_sub = Some(sub);
        self.artist_page = Some(page);
    }

    fn make_profile_page(&mut self, uid: i64, cx: &mut Context<Self>) {
        let my_uid = self.account_uid(cx);
        let page = cx.new(|cx| ProfilePage::new(cx, self.services.clone(), uid, my_uid));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self.profile_sub = Some(sub);
        self.profile_page = Some(page);
    }

    fn make_account_page(&mut self, cx: &mut Context<Self>) {
        let nickname_input = self.account_nickname_input.clone();
        let signature_input = self.account_signature_input.clone();
        let page = cx.new(|cx| AccountPage::new(cx, self.services.clone(), nickname_input, signature_input));
        let sub = cx.subscribe(&page, |root: &mut Root, _e, ev: &PageEvent, cx| {
            root.on_page_event(ev, cx)
        });
        self.account_sub = Some(sub);
        self.account_page = Some(page);
    }

    pub(crate) fn open_playlist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Playlist { id, name: name.clone() });
        self.make_playlist_page(id, name, cx);
        cx.notify();
    }

    pub(crate) fn open_album(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Album { id, name: name.clone() });
        self.make_album_page(id, name, cx);
        cx.notify();
    }

    fn open_artist(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        self.nav.push(Route::Artist { id, name: name.clone() });
        self.make_artist_page(id, name, cx);
        cx.notify();
    }

    fn open_profile(&mut self, uid: i64, cx: &mut Context<Self>) {
        self.nav.push(Route::Profile { uid });
        self.make_profile_page(uid, cx);
        cx.notify();
    }

    fn account_uid(&self, cx: &App) -> Option<i64> {
        Session::uid(cx)
    }

    fn open_account(&mut self, cx: &mut Context<Self>) {
        self.nav.push(Route::Account);
        self.make_account_page(cx);
        cx.notify();
    }

    /// 按当前路由（重新）建立详情页实体；顶层路由无需实体。
    ///
    /// `back`/`forward` 会先 `clear_detail_pages`：若不在此重建，回退/前进到详情路由时
    /// 实体已被清空、`page()` 只能渲染空白（多层详情导航/侧栏连点后返回即触发）。
    fn ensure_route_page(&mut self, cx: &mut Context<Self>) {
        match self.route() {
            Route::Playlist { id, name } => self.make_playlist_page(id, name, cx),
            Route::Album { id, name } => self.make_album_page(id, name, cx),
            Route::Artist { id, name } => self.make_artist_page(id, name, cx),
            Route::Profile { uid } => self.make_profile_page(uid, cx),
            Route::Account => self.make_account_page(cx),
            _ => {}
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.nav.pop() {
            self.clear_detail_pages();
            self.ensure_route_page(cx);
            cx.notify();
        }
    }

    /// 前进（标题栏前进按钮，浏览器语义）。
    fn forward(&mut self, cx: &mut Context<Self>) {
        if self.nav.forward() {
            self.clear_detail_pages();
            self.ensure_route_page(cx);
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
        // 一并注销详情页订阅（否则 `_subs` 只增不减，旧页面被订阅长期持有）。
        self.playlist_sub = None;
        self.album_sub = None;
        self.artist_sub = None;
        self.profile_sub = None;
        self.account_sub = None;
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

    /// 登录弹窗的默认提交动作（Enter）：仅 Cookie 标签有提交动作，其余标签忽略。
    pub(crate) fn submit_login_default(&mut self, cx: &mut Context<Self>) {
        if self.login.tab == login_dialog::LoginTab::Cookie {
            self.submit_cookie_login(cx);
        }
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
        // CSD（客户端装饰）：合成器不画标题栏，阴影/缩放区由 gpui-kit 的 Root
        // （gpui-component `window_border`）承担；此处仅做宽度补偿与按实际模式降级。
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let pads = gpui_kit::component::window_paddings(window);
        let vw = f32::from(window.viewport_size().width) - f32::from(pads.left) - f32::from(pads.right);
        let weak = cx.weak_entity();
        let weak_page = cx.weak_entity();
        let weak_login = cx.weak_entity();
        let weak_login_key = cx.weak_entity();
        let weak_comments = cx.weak_entity();
        let login_overlay = self.login.open.then(|| {
            let weak_key = weak_login_key.clone();
            div()
                .id("login-scope")
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h_full()
                // Enter 默认提交：焦点在 Cookie 输入框时由其自身订阅处理（避免重复提交）；
                // 焦点在弹窗内其它控件（如「登录」按钮）时由这里兜底提交。
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key != "enter" {
                        return;
                    }
                    let input_focused = weak_key
                        .update(cx, |root, cx| {
                            root.login.input.read(cx).focus_handle(cx).is_focused(window)
                        })
                        .unwrap_or(false);
                    if input_focused {
                        return;
                    }
                    weak_key.update(cx, |root, cx| root.submit_login_default(cx)).ok();
                })
                .child(self.login_overlay(weak_login))
                .into_any_element()
        });
        let comments_overlay = self
            .comments
            .open
            .then(|| self.comments_overlay(weak_comments, window, cx));
        let weak_create = cx.weak_entity();
        let weak_picker = cx.weak_entity();
        let create_overlay = self.ops.create_open.then(|| self.create_overlay(weak_create, cx));
        let picker_overlay = self
            .ops
            .picker_song
            .map(|_| self.picker_overlay(weak_picker, window, cx));
        let weak_rename = cx.weak_entity();
        let weak_delete = cx.weak_entity();
        let rename_overlay = self.ops.rename_pid.map(|_| self.rename_overlay(weak_rename, cx));
        let delete_overlay = self.ops.delete_pid.map(|_| self.delete_overlay(weak_delete));
        let weak_desc = cx.weak_entity();
        let desc_overlay = self.ops.desc_pid.map(|_| self.desc_overlay(weak_desc, cx));
        let weak_queue = cx.weak_entity();
        let queue_overlay = self.queue_open.then(|| self.queue_overlay(weak_queue, window, cx));
        // 中间列显式分配宽度，规避 gpui `flex_1` 的横向过度伸展。
        // 右侧栏取「有效宽」（随窗口收缩联动，见 np_effective_w）：空间不足时先压缩
        // 右栏自身显示宽，中间列保底最小内容宽，右栏不会被裁剪出窗口（N6.2）。
        let np_w = self.np_effective_w(vw, cx);
        let page_w = (vw - LIBRARY_W - np_w - SHELL_CHROME).max(0.0);
        cx.global_mut::<PageLayout>().page_w = page_w;
        let shell = div()
            .v_flex()
            .relative()
            .size_full()
            .font_family("MiSans")
            .bg(theme::window_bg())
            .child(self.title_bar(weak.clone(), vw, client_decorated, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .gap(px(theme::spacing::SM))
                    .px(px(theme::spacing::SM))
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
                    .child(
                        div()
                            .w(px(np_w))
                            .min_w(px(np_w))
                            .flex_none()
                            .h_full()
                            .overflow_hidden()
                            .child(self.now_playing(weak.clone(), window, cx)),
                    ),
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
            .child(self.notifications_overlay(cx));
        // CSD 的阴影 / 窗口边缘缩放区 / client inset 由 gpui-kit 的 Root
        // （gpui-component `WindowState` 插件 → `window_border`）统一承担，
        // 这里只需按 `window_paddings` 扣除内边距做布局补偿（见上方 `vw`）。
        shell
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
        .child(div().w_full().p(px(theme::spacing::MD)).child(header))
        .child(body)
}

/// 顶部收起因子：0 = 展开，1 = 收起。
///
/// 依据是内容区的平滑滚动偏移（[`smooth_scroll::smooth_scroll_offset`]），用 kit 的
/// `transition` 插值——可逆（滚回顶部自动展开），且自动尊重系统「减少动态效果」。
/// `id` 需**逐页唯一**：同一 view 内重复的元素 id 会触发 gpui 的 a11y 断言。
pub(crate) fn header_fold(id: &'static str, scroll_id: &'static str, window: &mut Window, cx: &mut App) -> f32 {
    let collapsed = smooth_scroll::smooth_scroll_offset(scroll_id, cx) > pages::dim::HEADER_COLLAPSE_AT_PX;
    transition(
        id,
        if collapsed { 1.0f32 } else { 0.0 },
        Transition::new(motion::HEADER_COLLAPSE).ease(motion::ease_in_out()),
        window,
        cx,
    )
}

/// 可折叠区外壳：高度随 `fold` 收缩到 0，并裁掉内容与淡出。
///
/// ⚠️ **`content` 必须自带 padding，外壳不能有**——外壳若带 padding，高度插值到 0 时
/// padding 仍占位，收起后会在横栏上方留一条空白。
pub(crate) fn collapse_box(fold: f32, full_h: f32, content: AnyElement) -> AnyElement {
    div()
        .w_full()
        .h(px(full_h * (1.0 - fold)))
        .overflow_hidden()
        .opacity(1.0 - fold)
        .child(content)
        .into_any_element()
}

pub(crate) fn fmt_time(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// 毫秒时间戳 → `M月D日`（本地时区；见 `siren_core::time`）。
pub(crate) fn fmt_month_day(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    match siren_core::time::local_date(ms) {
        Some((_, m, d, _)) => format!("{m}月{d}日"),
        None => String::new(),
    }
}

/// 毫秒时间戳 → 分区块标题（`8月24日 周一`；跨年补年份）。无时间返回空串。
///
/// 按**本地**日历日切分：原先用 `ms / 86_400_000` 是 UTC 切日，东八区凌晨的记录
/// 会被归到前一天（整改清单 K5）。
pub(crate) fn fmt_day_label(ms: i64, now_ms: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    if ms <= 0 {
        return String::new();
    }
    let (Some((y, m, d, weekday)), Some((ny, _, _, _))) =
        (siren_core::time::local_date(ms), siren_core::time::local_date(now_ms))
    else {
        return String::new();
    };
    let date = if y == ny {
        format!("{m}月{d}日")
    } else {
        format!("{y}年{m}月{d}日")
    };
    format!("{date} {}", WEEKDAYS[(weekday.clamp(1, 7) - 1) as usize])
}

/// 毫秒时间戳 → `YYYY-MM-DD`（本地时区；无日期时返回空串）。
pub(crate) fn fmt_date(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    match siren_core::time::local_date(ms) {
        Some((y, m, d, _)) => format!("{y}-{m:02}-{d:02}"),
        None => String::new(),
    }
}

pub(crate) fn icon(name: IconName, size: f32, color: Rgba) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

/// 加载失败占位：提示文案 + 重试按钮。
///
/// `retry` 传页面的重载方法（如 `HomePage::load`），用于区分「加载失败」与「空数据」（K8）。
pub(crate) fn load_error<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    message: &str,
    retry: fn(&mut E, &mut Context<E>),
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .py(px(theme::spacing::XL))
        .child(
            div()
                .text_sm()
                .text_color(theme::text_gray())
                .child(message.to_string()),
        )
        .child(Button::new(id).label("重试").on_click(move |_ev, _w, cx| {
            weak.update(cx, retry).ok();
        }))
        .into_any_element()
}

pub(crate) fn cover_placeholder(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
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
        Route::Playlist {
            id,
            name: format!("pl-{id}"),
        }
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
    fn push_caps_depth_and_drops_oldest() {
        let mut nav = NavState::new(Route::Home);
        for id in 1..=20 {
            nav.push(pl(id));
        }
        // 仅保留最近 10 条：pl(11)..=pl(20)；栈底 Home 与 pl(1..=10) 被丢弃。
        assert_eq!(nav.route(), pl(20));
        let mut depth = 1;
        while nav.pop() {
            depth += 1;
        }
        assert_eq!(depth, super::NAV_MAX_DEPTH);
        assert_eq!(nav.route(), pl(11));
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
