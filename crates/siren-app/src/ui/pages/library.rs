//! 音乐库：登录卡片 / 我的歌单。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use siren_core::model::Playlist;

use super::super::services::Services;
use super::super::{page_shell, PageEvent};
use crate::theme;

pub(crate) struct LibraryPage {
    services: Services,
    logged_in: bool,
    nickname: SharedString,
    login_input: Entity<InputState>,
    login_status: SharedString,
    my_playlists: Arc<Vec<Playlist>>,
    uid: Option<i64>,
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
            logged_in: false,
            nickname: "未登录".into(),
            login_input,
            login_status: "未登录".into(),
            my_playlists: Arc::new(Vec::new()),
            uid: None,
            _login_sub,
        };
        page.refresh_account(cx);
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
        // 共享客户端：登录态对 Playback / 封面加载器即时生效。
        self.services.client.set_cookie(Some(cookie));
        self.login_status = "登录校验中…".into();
        cx.notify();
        self.refresh_account(cx);
    }

    fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
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
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.logged_in {
            let playlists = self.my_playlists.clone();
            let count = playlists.len();
            let covers = self.services.covers.clone();
            let weak = cx.weak_entity();
            let list = uniform_list("lib-playlists", count, move |range, _window, _cx| {
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
                            .h(px(64.0))
                            .px(px(16.0))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .rounded(px(8.0))
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
            .min_w(px(0.0));

            page_shell(
                format!("音乐库 · {} · {}", self.nickname, self.login_status),
                list.into_any_element(),
            )
        } else {
            let weak = cx.weak_entity();
            let card = div()
                .v_flex()
                .gap(px(10.0))
                .w(px(440.0))
                .p(px(16.0))
                .rounded(px(10.0))
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
                        .child("粘贴 MUSIC_U 或完整 Cookie（也可用 SIREN_COOKIE 环境变量）"),
                )
                .child(Input::new(&self.login_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            Button::new("login")
                                .primary()
                                .label("登录")
                                .on_click(move |_ev, _window, cx| {
                                    weak.update(cx, |this, cx| this.do_login(cx)).ok();
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_gray())
                                .child(self.login_status.clone()),
                        ),
                );
            page_shell(
                "音乐库".to_string(),
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(card)
                    .into_any_element(),
            )
        }
    }
}
