//! 歌单写操作浮层：新建歌单、收藏歌曲到歌单。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::*;

use super::super::Root;
use crate::theme;

impl Root {
    /// 打开「新建歌单」浮层。
    pub(crate) fn open_create(&mut self, cx: &mut Context<Self>) {
        self.create_open = true;
        cx.notify();
    }

    pub(crate) fn close_create(&mut self, cx: &mut Context<Self>) {
        self.create_open = false;
        cx.notify();
    }

    /// 提交新建歌单。
    pub(crate) fn submit_create(&mut self, cx: &mut Context<Self>) {
        let name = self.create_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let id = siren_core::network::api::playlist_create(&client, &name, 0).await.ok();
            let _ = tx.send(id);
        });
        self.create_open = false;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let id = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if id.is_some() {
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 打开「添加到歌单」浮层（拉取我的歌单）。
    pub(crate) fn open_picker(&mut self, song_id: i64, cx: &mut Context<Self>) {
        self.picker_song = Some(song_id);
        self.picker_playlists.clear();
        cx.notify();
        let Some(uid) = self.account.as_ref().map(|(_, _, uid)| *uid) else {
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
        cx.spawn(async move |this, cx| {
            let list = rx.await.unwrap_or_default();
            this.update(cx, |this, cx| {
                let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                this.picker_playlists = list;
                this.services.covers.prefetch(urls, 48.0, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn close_picker(&mut self, cx: &mut Context<Self>) {
        self.picker_song = None;
        cx.notify();
    }

    /// 把待加入歌曲加到指定歌单。
    pub(crate) fn add_to_playlist(&mut self, pid: i64, cx: &mut Context<Self>) {
        let Some(song) = self.picker_song else {
            return;
        };
        let client = self.services.client.clone();
        self.services.handle.spawn(async move {
            let _ = siren_core::network::api::playlist_add_tracks(&client, pid, &[song]).await;
        });
        self.picker_song = None;
        cx.notify();
    }

    /// 新建歌单浮层。
    pub(crate) fn create_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak;
        div()
            .id("create-overlay")
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
                    .w(px(360.0))
                    .gap(px(14.0))
                    .rounded(px(12.0))
                    .bg(theme::surface())
                    .p(px(20.0))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("新建歌单"),
                    )
                    .child(Input::new(&self.create_input))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(Button::new("create-cancel").label("取消").on_click(move |_ev, _w, cx| {
                                weak_cancel.update(cx, |root, cx| root.close_create(cx)).ok();
                            }))
                            .child(
                                Button::new("create-ok")
                                    .primary()
                                    .label("创建")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_ok.update(cx, |root, cx| root.submit_create(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 添加到歌单浮层。
    pub(crate) fn picker_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let weak_close = weak.clone();
        let weak_create = weak.clone();
        let mut list = div()
            .id("picker-scroll")
            .v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .gap(px(4.0))
            .overflow_y_scroll();
        for pl in &self.picker_playlists {
            let weak = weak.clone();
            let pid = pl.id;
            list = list.child(
                div()
                    .id(format!("picker-{}", pl.id))
                    .w_full()
                    .h(px(48.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::card_surface()))
                    .on_click(move |_ev, _window, cx| {
                        weak.update(cx, |root, cx| root.add_to_playlist(pid, cx)).ok();
                    })
                    .child(self.services.covers.image(&pl.cover_url, 40.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_sm()
                            .text_color(theme::text_primary())
                            .child(pl.name.clone()),
                    ),
            );
        }
        div()
            .id("picker-overlay")
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
                    .w(px(360.0))
                    .h(px(500.0))
                    .gap(px(10.0))
                    .rounded(px(12.0))
                    .bg(theme::surface())
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
                                    .child("添加到歌单"),
                            )
                            .child(
                                div()
                                    .id("picker-create")
                                    .cursor_pointer()
                                    .text_sm()
                                    .text_color(theme::accent())
                                    .child("新建歌单")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_create
                                            .update(cx, |root, cx| {
                                                root.close_picker(cx);
                                                root.open_create(cx);
                                            })
                                            .ok();
                                    }),
                            ),
                    )
                    .child(list)
                    .child(Button::new("picker-cancel").label("取消").on_click(move |_ev, _w, cx| {
                        weak_close.update(cx, |root, cx| root.close_picker(cx)).ok();
                    })),
            )
            .into_any_element()
    }

    /// 打开重命名歌单浮层。
    pub(crate) fn open_rename(&mut self, id: i64, cx: &mut Context<Self>) {
        self.rename_pid = Some(id);
        cx.notify();
    }

    pub(crate) fn close_rename(&mut self, cx: &mut Context<Self>) {
        self.rename_pid = None;
        cx.notify();
    }

    /// 提交重命名。
    pub(crate) fn submit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.rename_pid else { return };
        let name = self.rename_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_rename(&client, id, &name)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        self.rename_pid = None;
        cx.notify();
        // 待接口返回后再刷新列表
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 打开删除确认浮层。
    pub(crate) fn open_delete(&mut self, id: i64, cx: &mut Context<Self>) {
        self.delete_pid = Some(id);
        cx.notify();
    }

    pub(crate) fn close_delete(&mut self, cx: &mut Context<Self>) {
        self.delete_pid = None;
        cx.notify();
    }

    pub(crate) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.delete_pid else { return };
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_delete(&client, id).await.is_ok();
            let _ = tx.send(ok);
        });
        self.delete_pid = None;
        cx.notify();
        // 待接口返回后再刷新列表
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 重命名浮层。
    pub(crate) fn rename_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak;
        div()
            .id("rename-overlay")
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
                    .w(px(360.0))
                    .gap(px(14.0))
                    .rounded(px(12.0))
                    .bg(theme::surface())
                    .p(px(20.0))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("重命名歌单"),
                    )
                    .child(Input::new(&self.rename_input))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(Button::new("rename-cancel").label("取消").on_click(move |_ev, _w, cx| {
                                weak_cancel.update(cx, |root, cx| root.close_rename(cx)).ok();
                            }))
                            .child(
                                Button::new("rename-ok")
                                    .primary()
                                    .label("确定")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_ok.update(cx, |root, cx| root.submit_rename(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 删除确认浮层。
    pub(crate) fn delete_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak;
        div()
            .id("delete-overlay")
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
                    .w(px(340.0))
                    .gap(px(14.0))
                    .rounded(px(12.0))
                    .bg(theme::surface())
                    .p(px(20.0))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("删除歌单"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child("确定删除该歌单？此操作不可撤销。"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(Button::new("delete-cancel").label("取消").on_click(move |_ev, _w, cx| {
                                weak_cancel.update(cx, |root, cx| root.close_delete(cx)).ok();
                            }))
                            .child(
                                Button::new("delete-ok")
                                    .danger()
                                    .label("删除")
                                    .on_click(move |_ev, _w, cx| {
                                        weak_ok.update(cx, |root, cx| root.confirm_delete(cx)).ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
