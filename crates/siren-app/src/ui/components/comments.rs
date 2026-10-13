//! 评论面板：右侧「查看全部评论」打开的浮层。
//!
//! 对齐 Melodia `CommentsPanel.kt` / `CommentFloorPanel.kt`（列表 / 楼层 / 点赞）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::*;

use siren_core::model::CommentItem;
use siren_core::network::api;

use super::super::{fmt_date, icon, Root};
use crate::theme;

impl Root {
    /// 打开评论面板（用播放器已载入的首页数据播种）。
    pub(crate) fn open_comments(&mut self, cx: &mut Context<Self>) {
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
        self.comments.open = true;
        let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
        self.services.covers.prefetch(avatars, 32.0, cx);
        cx.notify();
    }

    pub(crate) fn close_comments(&mut self, cx: &mut Context<Self>) {
        self.comments.open = false;
        cx.notify();
    }

    fn load_more_comments(&mut self, cx: &mut Context<Self>) {
        let Some(song) = self.playback.read(cx).state().song else {
            return;
        };
        let tid = api::comment_thread_id(song.id);
        let cursor = self.comments.cursor.clone();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let page = api::comments(&client, &tid, &cursor, 20).await.ok();
            let _ = tx.send(page);
        });
        cx.spawn(async move |this, cx| {
            let page = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if let Some(page) = page {
                    let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
                    this.comments.items.extend(page.comments);
                    this.comments.cursor = page.cursor;
                    this.comments.has_more = page.has_more;
                    this.services.covers.prefetch(avatars, 32.0, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_comment_like(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(song) = self.playback.read(cx).state().song else {
            return;
        };
        let tid = api::comment_thread_id(song.id);
        let mut like = false;
        if let Some(item) = self.comments.items.iter_mut().find(|c| c.comment_id == id) {
            item.liked = !item.liked;
            if item.liked {
                item.liked_count += 1;
            } else {
                item.liked_count = (item.liked_count - 1).max(0);
            }
            like = item.liked;
        }
        cx.notify();
        let client = self.services.client.clone();
        self.services.handle.spawn(async move {
            let _ = api::comment_like(&client, &tid, id, like).await;
        });
    }

    fn load_comment_floor(&mut self, parent_id: i64, cx: &mut Context<Self>) {
        if self.comments.floor_open == Some(parent_id) {
            self.comments.floor_open = None;
            self.comments.floor_items.clear();
            cx.notify();
            return;
        }
        let Some(song) = self.playback.read(cx).state().song else {
            return;
        };
        let tid = api::comment_thread_id(song.id);
        let client = self.services.client.clone();
        self.comments.floor_open = Some(parent_id);
        self.comments.floor_items.clear();
        cx.notify();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let page = api::comment_floor(&client, &tid, parent_id, 20).await.ok();
            let _ = tx.send(page);
        });
        cx.spawn(async move |this, cx| {
            let page = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if let Some(page) = page {
                    let avatars: Vec<String> = page.comments.iter().map(|c| c.user.avatar_url.clone()).collect();
                    this.comments.floor_items = page.comments;
                    this.services.covers.prefetch(avatars, 32.0, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 评论浮层。
    pub(crate) fn comments_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let mut list = div()
            .id("comments-scroll")
            .v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .gap(px(12.0))
            .overflow_y_scroll();

        for item in &self.comments.items {
            list = list.child(self.comment_row(item, weak.clone()));
            if self.comments.floor_open == Some(item.comment_id) {
                for reply in &self.comments.floor_items {
                    list = list.child(div().pl(px(40.0)).child(self.comment_row(reply, weak.clone())));
                }
            }
        }

        let footer: AnyElement = if self.comments.has_more {
            let weak_more = weak.clone();
            div()
                .w_full()
                .flex()
                .justify_center()
                .py(px(8.0))
                .child(
                    div()
                        .id("comments-more")
                        .cursor_pointer()
                        .text_sm()
                        .text_color(theme::accent())
                        .child("加载更多")
                        .on_click(move |_ev, _window, cx| {
                            weak_more.update(cx, |root, cx| root.load_more_comments(cx)).ok();
                        }),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let weak_close = weak.clone();
        div()
            .id("comments-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .child(
                div()
                    .v_flex()
                    .w(px(520.0))
                    .h(px(640.0))
                    .gap(px(10.0))
                    .rounded(px(12.0))
                    .bg(theme::pane())
                    .p(px(16.0))
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
                                div()
                                    .id("comments-close")
                                    .size(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::surface()))
                                    .on_click(move |_ev, _window, cx| {
                                        weak_close.update(cx, |root, cx| root.close_comments(cx)).ok();
                                    })
                                    .child(icon(IconName::X, 16.0, theme::text_gray())),
                            ),
                    )
                    .child(list)
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
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
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
                            .gap(px(16.0))
                            .child(
                                div()
                                    .id(format!("comment-like-{id}"))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
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
                            .child(
                                div()
                                    .id(format!("comment-floor-{id}"))
                                    .text_xs()
                                    .text_color(theme::text_gray())
                                    .cursor_pointer()
                                    .child(if replies > 0 {
                                        format!("{replies} 条回复")
                                    } else {
                                        String::new()
                                    })
                                    .on_click(move |_ev, _window, cx| {
                                        weak_floor.update(cx, |root, cx| root.load_comment_floor(id, cx)).ok();
                                    }),
                            ),
                    ),
            ),
        );
        let _ = &mut row;
        row.into_any_element()
    }
}
