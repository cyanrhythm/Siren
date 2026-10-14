//! 个人主页：用户资料 + 关注/取消关注 + 歌单/关注/粉丝列表。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{Playlist, UserBrief, UserProfile};

use super::super::services::Services;
use super::super::smooth_scroll::smooth_scroll;
use super::super::{icon, page_shell_with_header, PageEvent};
use crate::theme;

pub(crate) struct ProfilePage {
    services: Services,
    uid: i64,
    own: bool,
    profile: Option<UserProfile>,
    playlists: Arc<Vec<Playlist>>,
    follows: Vec<UserBrief>,
    followeds: Vec<UserBrief>,
    load_gen: u64,
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
        handle.spawn(async move {
            let profile = siren_core::network::api::user_detail(&client, uid).await.ok().flatten();
            let playlists = siren_core::network::api::user_playlists(&client, uid, 100, 0)
                .await
                .unwrap_or_default();
            let follows = siren_core::network::api::user_follows(&client, uid, 30)
                .await
                .unwrap_or_default();
            let followeds = siren_core::network::api::user_followeds(&client, uid, 30)
                .await
                .unwrap_or_default();
            let _ = tx.send((profile, playlists, follows, followeds));
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.load_gen != gen {
                    return;
                }
                if let Some((profile, playlists, follows, followeds)) = payload {
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
                        this.services.toast_error("操作失败", cx);
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
            .gap(px(16.0))
            .child(avatar)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(6.0))
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
                            Button::new("profile-follow")
                                .primary()
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
                    .size(px(30.0))
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
            let cover = covers.image(&pl.cover_url, 48.0);
            let weak = weak.clone();
            let (id, name) = (pl.id, pl.name.clone());
            col = col.child(
                div()
                    .id(format!("profile-pl-{id}"))
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
            let avatar = covers.image_round(&u.avatar_url, 40.0);
            let weak = weak.clone();
            let id = u.user_id;
            col = col.child(
                div()
                    .id(format!("{prefix}-{id}"))
                    .w_full()
                    .h(px(52.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .rounded(px(6.0))
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
        let mut body = div().v_flex().w_full().p(px(16.0)).gap(px(18.0));

        body = body.child(section(
            "歌单",
            if self.playlists.is_empty() {
                empty_hint("暂无歌单")
            } else {
                self.playlist_rows(cx)
            },
        ));
        body = body.child(section(
            "关注",
            if self.follows.is_empty() {
                empty_hint("暂无关注")
            } else {
                self.users_rows(&self.follows, "profile-follow", cx)
            },
        ));
        body = body.child(section(
            "粉丝",
            if self.followeds.is_empty() {
                empty_hint("暂无粉丝")
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
        .gap(px(8.0))
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

fn empty_hint(text: &str) -> AnyElement {
    div()
        .w_full()
        .py(px(12.0))
        .text_xs()
        .text_color(theme::text_gray())
        .child(text.to_string())
        .into_any_element()
}
