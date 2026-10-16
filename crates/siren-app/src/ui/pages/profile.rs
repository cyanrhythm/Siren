//! 个人主页：用户资料 + 关注/取消关注 + 歌单/关注/粉丝列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Playlist, UserBrief, UserProfile};

use super::super::services::Services;
use super::super::smooth_scroll::smooth_scroll;
use super::super::{hint, icon, load_error, page_shell_with_header, PageEvent};
use super::dim;
use crate::theme::{self, spacing};

pub(crate) struct ProfilePage {
    services: Services,
    uid: i64,
    own: bool,
    profile: Option<UserProfile>,
    playlists: Arc<Vec<Playlist>>,
    follows: Vec<UserBrief>,
    followeds: Vec<UserBrief>,
    load_gen: u64,
    /// 主接口全部失败（离线等）：展示失败态而非空数据（K8）。
    failed: bool,
    /// 是否已拿到响应（区分「加载中」与真空数据）。
    loaded: bool,
}

impl EventEmitter<PageEvent> for ProfilePage {}

impl ProfilePage {
    pub(crate) fn new(cx: &mut Context<Self>, services: Services, uid: i64, my_uid: Option<i64>) -> Self {
        let mut page = Self {
            services,
            uid,
            own: my_uid == Some(uid),
            profile: None,
            playlists: Arc::new(Vec::new()),
            follows: Vec::new(),
            followeds: Vec::new(),
            load_gen: 0,
            failed: false,
            loaded: false,
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_gen = self.load_gen.wrapping_add(1);
        let gen = self.load_gen;
        let client = self.services.client.clone();
        let uid = self.uid;
        let handle = self.services.handle.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.failed = false;
        handle.spawn(async move {
            let profile = siren_core::network::api::user_detail(&client, uid).await;
            let playlists = siren_core::network::api::user_playlists(&client, uid, 100, 0).await;
            let follows = siren_core::network::api::user_follows(&client, uid, 30).await;
            let followeds = siren_core::network::api::user_followeds(&client, uid, 30).await;
            // 资料与歌单均失败视为整体失败（离线）；单项失败仍展示其余内容（K8）。
            let failed = profile.is_err() && playlists.is_err();
            let _ = tx.send((
                profile.ok().flatten(),
                playlists.unwrap_or_default(),
                follows.unwrap_or_default(),
                followeds.unwrap_or_default(),
                failed,
            ));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((profile, playlists, follows, followeds, failed)) = payload {
                    this.failed = failed;
                    this.loaded = true;
                    if let Some(p) = &profile {
                        this.services.covers.prefetch([p.avatar_url.clone()], 108.0, cx);
                    }
                    this.services
                        .covers
                        .prefetch(playlists.iter().map(|p| p.cover_url.clone()), 48.0, cx);
                    this.services
                        .covers
                        .prefetch(follows.iter().map(|u| u.avatar_url.clone()), 40.0, cx);
                    this.services
                        .covers
                        .prefetch(followeds.iter().map(|u| u.avatar_url.clone()), 40.0, cx);
                    this.profile = profile;
                    this.playlists = Arc::new(playlists);
                    this.follows = follows;
                    this.followeds = followeds;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 关注 / 取关；乐观更新，失败回滚并 Toast。
    fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        let Some(profile) = self.profile.as_mut() else {
            return;
        };
        let follow = !profile.followed;
        profile.followed = follow;
        cx.notify();
        let client = self.services.client.clone();
        let uid = self.uid;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::user_follow(&client, uid, follow)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if let Some(p) = this.profile.as_mut() {
                    if !ok {
                        p.followed = !follow;
                        this.services.toast_error("关注用户失败，请重试", cx);
                    } else {
                        this.services.toast(if follow { "已关注" } else { "已取消关注" }, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn header(&self, cx: &Context<Self>) -> AnyElement {
        let weak_back = cx.weak_entity();
        let weak_follow = cx.weak_entity();
        let (avatar_url, nickname, signature, follows, followeds, followed) = match &self.profile {
            Some(p) => (
                p.avatar_url.clone(),
                p.nickname.clone(),
                p.signature.clone(),
                p.follows,
                p.followeds,
                p.followed,
            ),
            None => (String::new(), "加载中…".to_string(), String::new(), 0, 0, false),
        };
        let avatar = self.services.covers.image_round(&avatar_url, 108.0);
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::LG))
            .child(avatar)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(spacing::XXS))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .truncate()
                            .child(nickname),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .truncate()
                            .child(signature),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child(format!("{follows} 关注 · {followeds} 粉丝")),
                    )
                    .when(!self.own, |el| {
                        el.child(
                            // 「已关注」用中性按钮（与歌手页一致），不再用主色强调。
                            Button::new("profile-follow")
                                .label(if followed { "已关注" } else { "关注" })
                                .on_click(move |_ev, _window, cx| {
                                    weak_follow.update(cx, |this, cx| this.toggle_follow(cx)).ok();
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .id("profile-back")
                    .size(px(dim::ROUND_BTN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak_back.update(cx, |_p, cx| cx.emit(PageEvent::Back)).ok();
                    })
                    .child(icon(IconName::ChevronLeft, 18.0, theme::text_gray())),
            )
            .into_any_element()
    }

    fn playlist_rows(&self, cx: &Context<Self>) -> AnyElement {
        let playlists = self.playlists.clone();
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for pl in playlists.iter() {
            let cover = covers.image(&pl.cover_url, dim::COVER_LIST);
            let weak = weak.clone();
            let (id, name) = (pl.id, pl.name.clone());
            col = col.child(
                div()
                    .id(format!("profile-pl-{id}"))
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
                        weak.update(cx, |_p, cx| {
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
                    ),
            );
        }
        col.into_any_element()
    }

    fn users_rows(&self, users: &[UserBrief], prefix: &'static str, cx: &Context<Self>) -> AnyElement {
        let covers = self.services.covers.clone();
        let weak = cx.weak_entity();
        let mut col = div().v_flex().w_full();
        for u in users {
            let avatar = covers.image_round(&u.avatar_url, dim::COVER_SONG);
            let weak = weak.clone();
            let id = u.user_id;
            col = col.child(
                div()
                    .id(format!("{prefix}-{id}"))
                    .w_full()
                    .h(px(dim::ROW_H))
                    .px(px(spacing::XXS))
                    .flex()
                    .items_center()
                    .gap(px(spacing::MD))
                    .rounded(px(theme::RADIUS_SM))
                    .hover(|s| s.bg(theme::card_surface()))
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |_p, cx| {
                            cx.emit(PageEvent::OpenProfile { uid: id });
                        })
                        .ok();
                    })
                    .child(avatar)
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
                                    .child(u.nickname.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .truncate()
                                    .child(u.signature.clone()),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }
}

impl Render for ProfilePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let mut body = div().v_flex().w_full().p(px(spacing::LG)).gap(px(18.0));
        if self.failed {
            body = body.child(load_error(
                "profile-retry",
                cx.weak_entity(),
                "个人主页加载失败",
                ProfilePage::load,
            ));
        }

        // 未返回时显示「加载中」，返回后真空才显示「暂无 XX」。
        body = body.child(section(
            "歌单",
            if !self.loaded {
                hint("加载中…")
            } else if self.playlists.is_empty() {
                hint("暂无歌单")
            } else {
                self.playlist_rows(cx)
            },
        ));
        body = body.child(section(
            "关注",
            if !self.loaded {
                hint("加载中…")
            } else if self.follows.is_empty() {
                hint("暂无关注")
            } else {
                self.users_rows(&self.follows, "profile-follow", cx)
            },
        ));
        body = body.child(section(
            "粉丝",
            if !self.loaded {
                hint("加载中…")
            } else if self.followeds.is_empty() {
                hint("暂无粉丝")
            } else {
                self.users_rows(&self.followeds, "profile-fans", cx)
            },
        ));

        page_shell_with_header(
            header,
            smooth_scroll("profile-scroll", body, window, cx).into_any_element(),
        )
    }
}

fn section(title: &str, body: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(spacing::SM))
        .child(
            div()
                .text_lg()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(body)
        .into_any_element()
}
