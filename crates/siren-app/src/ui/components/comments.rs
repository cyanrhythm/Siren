//! 评论面板：右侧「查看全部评论」打开的浮层。
//!
//! 对齐 Melodia `CommentsPanel.kt` / `CommentFloorPanel.kt`（列表 / 楼层 / 点赞）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::Disableable as _;
use gpui_kit::*;

use siren_core::error::ResultExt;
use siren_core::model::CommentItem;
use siren_core::network::api;

use super::super::smooth_scroll::smooth_scroll;
use super::super::{fmt_date, icon, Root};
use super::collect::{overlay_focus, overlay_scrim};
use crate::theme::{self, spacing};

impl Root {
    /// 打开评论面板（用播放器已载入的首页数据播种）。
    pub(crate) fn open_comments(&mut self, cx: &mut Context<Self>) {
        overlay_focus("comments-overlay", cx);
        let page = self.playback.read(cx).comments().clone();
        self.comments.items = page.comments.clone();
        self.comments.total = page.total;
        self.comments.has_more = page.has_more;
        self.comments.cursor = if page.cursor.is_empty() {
            "0".into()
        } else {
            page.cursor
        };
        self.comments.floor_open = None;
        self.comments.floor_items.clear();
        self.comments.song_id = self.playback.read(cx).current_song_id();
        self.comments.loading_more = false;
        self.comments.open = true;
        let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
        self.services.covers.prefetch(avatars, 32.0, cx);
        cx.notify();
    }

    pub(crate) fn close_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.comments.open = false;
        window.blur(cx);
        cx.notify();
    }

    fn load_more_comments(&mut self, cx: &mut Context<Self>) {
        // 在途守卫：连点不重复分页。
        if self.comments.loading_more {
            return;
        }
        let Some(owner) = self.playback.read(cx).current_song_id() else {
            return;
        };
        self.comments.loading_more = true;
        let tid = api::comment_thread_id(owner);
        let cursor = self.comments.cursor.clone();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let page = api::comments(&client, &tid, &cursor, 20).await.ok_or_warn("加载评论");
            let _ = tx.send(page);
        });
        cx.spawn(async move |this, cx| {
            let page = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.comments.loading_more = false;
                // 归属校验：期间切歌则丢弃，避免新旧歌评论混列表。
                if this.comments.song_id != Some(owner) {
                    cx.notify();
                    return;
                }
                if let Some(page) = page {
                    let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
                    this.comments.items.extend(page.comments);
                    this.comments.cursor = page.cursor;
                    this.comments.has_more = page.has_more;
                    this.services.covers.prefetch(avatars, 32.0, cx);
                } else {
                    log::warn!("加载评论失败：接口无数据或任务未返回");
                    this.services.toast_error("评论加载失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_comment_like(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(song_id) = self.playback.read(cx).current_song_id() else {
            return;
        };
        let tid = api::comment_thread_id(song_id);
        // 顶层评论与楼层回复都要查（回复在 floor_items 中）；都没找到就不发请求，
        // 避免对从未喜欢过的评论发出「取消喜欢」。
        let flip = |item: &mut CommentItem| {
            item.liked = !item.liked;
            if item.liked {
                item.liked_count += 1;
            } else {
                item.liked_count = (item.liked_count - 1).max(0);
            }
            item.liked
        };
        let like = if let Some(item) = self.comments.items.iter_mut().find(|c| c.comment_id == id) {
            flip(item)
        } else if let Some(item) = self.comments.floor_items.iter_mut().find(|c| c.comment_id == id) {
            flip(item)
        } else {
            return;
        };
        cx.notify();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(api::comment_like(&client, &tid, id, like).await);
        });
        // 失败回滚本地喜欢态与计数（对齐 playback.rs toggle_like_song 的乐观+回滚）。
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(())) => {}
                    Some(Err(err)) => {
                        if let Some(item) = this.comments.items.iter_mut().find(|c| c.comment_id == id) {
                            flip(item);
                        } else if let Some(item) = this.comments.floor_items.iter_mut().find(|c| c.comment_id == id) {
                            flip(item);
                        }
                        this.services.toast_error(err.user_message(), cx);
                    }
                    None => {
                        log::warn!("评论点赞：任务未返回（通道断开）");
                        this.services.toast_error("操作失败，请重试", cx)
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_comment_floor(&mut self, parent_id: i64, cx: &mut Context<Self>) {
        if self.comments.floor_open == Some(parent_id) {
            self.comments.floor_open = None;
            self.comments.floor_items.clear();
            cx.notify();
            return;
        }
        let Some(owner) = self.playback.read(cx).current_song_id() else {
            return;
        };
        let tid = api::comment_thread_id(owner);
        let client = self.services.client.clone();
        self.comments.floor_open = Some(parent_id);
        self.comments.floor_items.clear();
        cx.notify();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let page = api::comment_floor(&client, &tid, parent_id, 20)
                .await
                .ok_or_warn("加载楼层评论");
            let _ = tx.send(page);
        });
        cx.spawn(async move |this, cx| {
            let page = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                // 归属校验：期间切歌则丢弃楼层结果。
                if this.comments.song_id != Some(owner) || this.comments.floor_open != Some(parent_id) {
                    return;
                }
                if let Some(page) = page {
                    let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
                    this.comments.floor_items = page.comments;
                    this.services.covers.prefetch(avatars, 32.0, cx);
                } else {
                    log::warn!("加载楼层评论失败：接口无数据或任务未返回");
                    this.services.toast_error("回复加载失败，请重试", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 评论浮层。
    pub(crate) fn comments_overlay(
        &self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div().v_flex().w_full().gap(px(spacing::MD));

        for item in &self.comments.items {
            list = list.child(self.comment_row(item, weak.clone()));
            if self.comments.floor_open == Some(item.comment_id) {
                for reply in &self.comments.floor_items {
                    list = list.child(div().pl(px(40.0)).child(self.comment_row(reply, weak.clone())));
                }
            }
        }
        if self.comments.items.is_empty() {
            list = list.child(
                div()
                    .w_full()
                    .py(px(spacing::XXL))
                    .flex()
                    .justify_center()
                    .child(div().text_sm().text_color(theme::text_gray()).child("暂无评论")),
            );
        }

        let footer: AnyElement = if self.comments.has_more {
            let loading = self.comments.loading_more;
            let weak_more = weak.clone();
            div()
                .w_full()
                .flex()
                .justify_center()
                .py(px(spacing::SM))
                .child(
                    Button::new("comments-more")
                        .ghost()
                        .label(if loading { "加载中…" } else { "加载更多" })
                        .loading(loading)
                        .disabled(loading)
                        .on_click(move |_ev, _window, cx| {
                            weak_more.update(cx, |root, cx| root.load_more_comments(cx)).ok();
                        }),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let weak_close = weak.clone();
        overlay_scrim("comments-overlay", weak, Root::close_comments)
            .child(
                div()
                    .v_flex()
                    .w(px(520.0))
                    .h(px(640.0))
                    .gap(px(10.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::pane())
                    .p(px(spacing::MD))
                    .id("comments-panel")
                    .on_click(move |_ev, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child(format!("评论 ({})", self.comments.total)),
                            )
                            .child(
                                Button::new("comments-close")
                                    .ghost()
                                    .icon(icon(IconName::X, 16.0, theme::text_gray()))
                                    .tooltip("关闭")
                                    .accessibility_label("关闭评论")
                                    .on_click(move |_ev, window, cx| {
                                        weak_close.update(cx, |root, cx| root.close_comments(window, cx)).ok();
                                    }),
                            ),
                    )
                    .child(smooth_scroll("comments-scroll", list, window, cx))
                    .child(footer),
            )
            .into_any_element()
    }

    fn comment_row(&self, item: &CommentItem, weak: WeakEntity<Self>) -> AnyElement {
        let avatar = self.services.covers.image_round(&item.user.avatar_url, 32.0);
        let weak_like = weak.clone();
        let weak_floor = weak;
        let id = item.comment_id;
        let liked = item.liked;
        let replies = item.reply_count;

        let mut row = div().v_flex().w_full().gap(px(6.0)).child(
            div().w_full().flex().items_start().gap(px(10.0)).child(avatar).child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(spacing::XS))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_gray())
                                    .child(item.user.nickname.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .child(fmt_date(item.time)),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .child(item.content.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(spacing::MD))
                            .child(
                                div()
                                    .id(format!("comment-like-{id}"))
                                    .flex()
                                    .items_center()
                                    .gap(px(spacing::XS))
                                    .cursor_pointer()
                                    .on_click(move |_ev, _window, cx| {
                                        weak_like.update(cx, |root, cx| root.toggle_comment_like(id, cx)).ok();
                                    })
                                    .child(icon(
                                        IconName::ThumbsUp,
                                        14.0,
                                        if liked { theme::accent() } else { theme::text_gray() },
                                    ))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::text_gray())
                                            .child(item.liked_count.to_string()),
                                    ),
                            )
                            .child(if replies > 0 {
                                Button::new(format!("comment-floor-{id}"))
                                    .ghost()
                                    .label(format!("{replies} 条回复"))
                                    .on_click(move |_ev, _window, cx| {
                                        weak_floor.update(cx, |root, cx| root.load_comment_floor(id, cx)).ok();
                                    })
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    ),
            ),
        );
        let _ = &mut row;
        row.into_any_element()
    }
}
