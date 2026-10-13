//! 音乐库（常驻侧栏）：头部工具栏 + 歌单列表 / 登录卡片。
//!
//! 头部对齐 Melodia 的库面板：标题「音乐库」+ 新建/展开；分栏 歌单/专辑/艺人；
//! 库内搜索 + 排序。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use siren_core::model::Playlist;

use super::super::services::Services;
use super::super::session::{Account, Session};
use super::super::{icon, PageEvent};
use crate::theme;

pub(crate) struct LibraryPage {
    services: Services,
    /// 登录表单输入与状态（视图态；账号身份存于 [`Session`]）。
    login_input: Entity<InputState>,
    login_status: SharedString,
    my_playlists: Arc<Vec<Playlist>>,
    load_gen: u64,
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
            login_status: "未登录".into(),
            my_playlists: Arc::new(Vec::new()),
            load_gen: 0,
            _login_sub,
        };
        page.refresh_account(false, cx);
        page
    }

    fn do_login(&mut self, cx: &mut Context<Self>) {
        let raw = self.login_input.read(cx).value().trim().to_string();
        let Some(cookie) = siren_core::auth::normalize_cookie(&raw) else {
            self.login_status = "请输入 MUSIC_U 或完整 Cookie".into();
            cx.notify();
            return;
        };
        siren_core::auth::save_cookie(&cookie);
        self.services.client.set_cookie(Some(cookie));
        self.login_status = "登录校验中…".into();
        cx.notify();
        self.refresh_account(true, cx);
    }

    /// 退出登录：清除 Cookie 与会话，回到未登录态。
    pub(crate) fn logout(&mut self, cx: &mut Context<Self>) {
        siren_core::auth::clear_cookie();
        self.services.client.set_cookie(None);
        Session::set(cx, None);
        self.login_status = "未登录".into();
        self.my_playlists = Arc::new(Vec::new());
        cx.emit(PageEvent::LoggedOut);
        cx.notify();
    }

    pub(crate) fn refresh_account(&mut self, announce: bool, cx: &mut Context<Self>) {
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
                        this.login_status = format!("已登录：{nickname}").into();
                        log::info!("已登录：{nickname} (uid={uid})");
                        if announce {
                            this.services.toast_success(format!("登录成功：{nickname}"), cx);
                        }
                        cx.emit(PageEvent::SignedIn);
                        this.load_my_playlists(cx);
                    }
                    None => {
                        Session::set(cx, None);
                        this.login_status = "未登录（Cookie 无效或已过期）".into();
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
        self.login_status = "加载我的歌单…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                this.login_status = format!("我的歌单 {} 个", list.len()).into();
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.my_playlists = Arc::new(list);
                this.services.covers.prefetch(urls, 48.0, cx);
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

    fn tabs(&self) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pb(px(6.0))
            .child(lib_tab("歌单", true))
            .child(lib_tab("专辑", false))
            .child(lib_tab("艺人", false))
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
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logged_in = Session::account(cx).is_some();
        let content: AnyElement =
            if logged_in {
                let playlists = self.my_playlists.clone();
                let count = playlists.len();
                let covers = self.services.covers.clone();
                let weak = cx.weak_entity();
                uniform_list("lib-playlists", count, move |range, _window, _cx| {
                    range
                        .map(|i| {
                            let pl = &playlists[i];
                            let cover = covers.image(&pl.cover_url, 48.0);
                            let weak = weak.clone();
                            let id = pl.id;
                            let name = pl.name.clone();
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
                })
                .flex_1()
                .w_full()
                .min_w(px(0.0))
                .into_any_element()
            } else {
                let weak = cx.weak_entity();
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap(px(8.0))
                            .p(px(12.0))
                            .rounded(px(8.0))
                            .bg(theme::card_surface())
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child("登录后查看你的歌单"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(self.login_status.clone()),
                            )
                            .child(Button::new("lib-login").primary().label("登录").on_click(
                                move |_ev, _window, cx| {
                                    weak.update(cx, |_page, cx| cx.emit(PageEvent::OpenLogin)).ok();
                                },
                            )),
                    )
                    .into_any_element()
            };

        div()
            .v_flex()
            .size_full()
            .rounded(px(8.0))
            .bg(theme::pane())
            .p(px(12.0))
            .gap(px(2.0))
            .child(self.header(cx.weak_entity()))
            .child(self.tabs())
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

fn lib_tab(label: &str, active: bool) -> impl IntoElement {
    div()
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
        .child(
            div()
                .text_sm()
                .text_color(theme::text_primary())
                .child(label.to_string()),
        )
}
