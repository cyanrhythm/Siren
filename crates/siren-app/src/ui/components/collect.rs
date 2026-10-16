//! 歌单写操作浮层：新建歌单、收藏歌曲到歌单。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use gpui_kit::base::{ElementExt as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::*;

use super::super::session::Session;
use super::super::smooth_scroll::smooth_scroll;
use super::super::{Root, Route, PLAYLIST_PAGE};
use crate::theme::{self, spacing};

thread_local! {
    /// 各浮层的焦点句柄：懒建后稳定复用，track_focus 依赖它把按键送达浮层根。
    static OVERLAY_FOCUS: RefCell<HashMap<&'static str, FocusHandle>> = RefCell::new(HashMap::new());
    /// 本次打开已把焦点收到浮层根上的浮层：保证只收一次，不抢浮层内输入框的焦点。
    static OVERLAY_FOCUS_TAKEN: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
    /// 各浮层打开**之前**的焦点：关闭时归还（无障碍要求「焦点在浮层关闭后回到原处」）。
    static OVERLAY_PREV_FOCUS: RefCell<HashMap<&'static str, Option<FocusHandle>>> =
        RefCell::new(HashMap::new());
    /// 删除确认浮层点名的歌单名（PlaylistOpsState 在 ui/mod.rs，跨 agent 不便加字段，
    /// 浮层同时只开一个，模块内暂存即可）。
    static DELETE_PNAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 取（或建）某浮层的焦点句柄；open_* 时预热，构建浮层时经 `overlay_focus_cached` 取。
///
/// 顺便清掉「本浮层已收过焦点」的标记：下次打开时会重新把焦点收到浮层根上（见 `overlay_scrim`）。
pub(crate) fn overlay_focus(id: &'static str, cx: &mut App) -> FocusHandle {
    OVERLAY_FOCUS_TAKEN.with(|set| set.borrow_mut().remove(id));
    OVERLAY_FOCUS.with(|map| map.borrow_mut().entry(id).or_insert_with(|| cx.focus_handle()).clone())
}

fn overlay_focus_cached(id: &'static str) -> Option<FocusHandle> {
    OVERLAY_FOCUS.with(|map| map.borrow().get(&id).cloned())
}

/// 浮层遮罩根：全屏压暗，点遮罩 / Esc 关闭；焦点为空时兜底聚焦浮层根（Esc 才能送达）。
///
/// 泛型 `E` 为浮层的 owner——Root 的浮层（新建/加入/重命名/删除/简介/评论）与页面的浮层
/// （歌单页的移除歌曲/移除歌单/封面裁剪/添加歌曲、队列清空）共用同一实现。
pub(crate) fn overlay_scrim<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
) -> Stateful<Div> {
    overlay_scrim_with(id, weak, close, theme::scrim())
}

/// 同 [`overlay_scrim`]，但压暗更重——用于封面裁剪这类需要突出前景的浮层。
pub(crate) fn overlay_scrim_strong<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
) -> Stateful<Div> {
    overlay_scrim_with(id, weak, close, theme::scrim_strong())
}

/// 同 [`overlay_scrim`]，但打开后把焦点直接落到浮层内的输入框（`inner`），
/// 用户打开即可输入，无需先点一下输入框（整改清单 O0.2）。
///
/// `reset_input` 非空时，打开的首帧会把该输入框清空——避免上次没提交就关掉时
/// 残留的文字带到下一次打开。
pub(crate) fn overlay_scrim_focusing<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
    inner: FocusHandle,
    reset_input: Option<Entity<InputState>>,
) -> Stateful<Div> {
    overlay_scrim_inner(id, weak, close, theme::scrim(), Some(inner), reset_input)
}

fn overlay_scrim_with<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
    scrim: Rgba,
) -> Stateful<Div> {
    overlay_scrim_inner(id, weak, close, scrim, None, None)
}

/// 关闭浮层时调用：把焦点归还到打开前的位置（没有记录时退回「清空焦点」）。
///
/// 浮层的 `close_*` 方法里用它替换原来的 `window.blur(cx)`。
pub(crate) fn overlay_close(id: &'static str, window: &mut Window, cx: &mut App) {
    match OVERLAY_PREV_FOCUS.with(|map| map.borrow_mut().remove(id)).flatten() {
        Some(prev) => window.focus(&prev, cx),
        None => window.blur(cx),
    }
}

fn overlay_scrim_inner<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
    scrim: Rgba,
    inner: Option<FocusHandle>,
    reset_input: Option<Entity<InputState>>,
) -> Stateful<Div> {
    let weak_click = weak.clone();
    let weak_esc = weak;
    let close_click = close.clone();
    let close_esc = close;
    let mut root = div()
        .id(id)
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h_full()
        // 浮层根会 `track_focus` 收键盘，故必须给 role——只有 id 没有 role 时
        // gpui 会警告「focused element has no accessibility node」（N0.4 / O1.12）。
        .role(Role::Dialog)
        .flex()
        .items_center()
        .justify_center()
        .bg(scrim)
        .on_click(move |ev: &ClickEvent, window, cx| {
            // 只响应鼠标左键；浮层根聚焦时 Enter/Space 产生的键盘 click 不关浮层。
            let on_scrim = matches!(ev, ClickEvent::Mouse(e) if e.up.button == MouseButton::Left);
            if on_scrim {
                weak_click.update(cx, |entity, cx| close_click(entity, window, cx)).ok();
            }
        })
        .on_key_down(move |ev: &KeyDownEvent, window, cx| {
            if ev.keystroke.key == "escape" {
                weak_esc.update(cx, |entity, cx| close_esc(entity, window, cx)).ok();
                cx.stop_propagation();
            }
        })
        .on_prepaint(move |_, window, cx| {
            // 打开后**第一帧**把焦点收到浮层根上（或浮层内的输入框），Esc 才能送达。
            //
            // 不能只在「当前无焦点」时收：kit Button 按下会 `prevent_default` 且不夺焦，
            // 若打开浮层前焦点在标题栏搜索框，焦点就一直留在那里，浮层根的 `on_key_down`
            // 收不到 Esc（整改清单 O0.2）。这里用「本浮层是否已收过焦点」判据，
            // 保证只收一次——之后用户点到别处不会被抢回去。
            if OVERLAY_FOCUS_TAKEN.with(|set| set.borrow_mut().insert(id)) {
                // 记录打开前的焦点，关闭时归还（无障碍：浮层关闭后焦点不应凭空消失）。
                OVERLAY_PREV_FOCUS.with(|map| map.borrow_mut().insert(id, window.focused(cx)));
                // 带输入框的浮层顺手清空上次残留（上次没提交就关掉时留在框里的文字）。
                if let Some(input) = reset_input.clone() {
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
                let focus = inner.clone().unwrap_or_else(|| overlay_focus(id, cx));
                window.focus(&focus, cx);
            }
        });
    if let Some(focus) = overlay_focus_cached(id) {
        root = root.track_focus(&focus);
    }
    root
}

impl Root {
    /// 打开「新建歌单」浮层。
    pub(crate) fn open_create(&mut self, cx: &mut Context<Self>) {
        overlay_focus("create-overlay", cx);
        self.ops.create_open = true;
        cx.notify();
    }

    pub(crate) fn close_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.create_open = false;
        overlay_close("create-overlay", window, cx);
        cx.notify();
    }

    /// 提交新建歌单。
    pub(crate) fn submit_create(&mut self, cx: &mut Context<Self>) {
        let name = self.ops.create_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.services.toast_error("歌单名不能为空", cx);
            return;
        }
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let id = siren_core::network::api::playlist_create(&client, &name, 0).await.ok();
            let _ = tx.send(id);
        });
        self.ops.create_open = false;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let id = rx.await.ok().flatten();
            this.update(cx, |this, cx| {
                if id.is_some() {
                    this.services.toast_success("歌单已创建", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                } else {
                    this.services.toast_error("创建歌单失败", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 打开「添加到歌单」浮层（拉取我的歌单）。
    pub(crate) fn open_picker(&mut self, song_id: i64, cx: &mut Context<Self>) {
        overlay_focus("picker-overlay", cx);
        self.ops.picker_song = Some(song_id);
        self.ops.picker_playlists.clear();
        self.ops.picker_offset = 0;
        self.ops.picker_more = false;
        cx.notify();
        let Some(uid) = Session::uid(cx) else {
            return;
        };
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::user_playlists(&client, uid, PLAYLIST_PAGE, 0).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(list)) => {
                        this.ops.picker_offset = list.len() as u32;
                        this.ops.picker_more = list.len() as u32 >= PLAYLIST_PAGE;
                        let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                        this.ops.picker_playlists = list;
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => this.services.toast_error("加载歌单失败，请重试", cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 追加下一页「加入歌单」候选（K1：超过一页可继续加载）。
    fn load_more_picker(&mut self, cx: &mut Context<Self>) {
        if !self.ops.picker_more {
            return;
        }
        let Some(uid) = Session::uid(cx) else {
            return;
        };
        let offset = self.ops.picker_offset;
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::user_playlists(&client, uid, PLAYLIST_PAGE, offset).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.ok();
            this.update(cx, |this, cx| {
                match result {
                    Some(Ok(list)) => {
                        this.ops.picker_offset += list.len() as u32;
                        this.ops.picker_more = list.len() as u32 >= PLAYLIST_PAGE;
                        let urls: Vec<String> = list.iter().map(|p| p.cover_url.clone()).collect();
                        this.ops.picker_playlists.extend(list);
                        this.services.covers.prefetch(urls, 48.0, cx);
                    }
                    Some(Err(err)) => this.services.toast_error(err.user_message(), cx),
                    None => this.services.toast_error("加载歌单失败，请重试", cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.picker_song = None;
        overlay_close("picker-overlay", window, cx);
        cx.notify();
    }

    /// 把待加入歌曲加到指定歌单，成功后刷新音乐库歌单列表（封面/数量）。
    pub(crate) fn add_to_playlist(&mut self, pid: i64, cx: &mut Context<Self>) {
        let Some(song) = self.ops.picker_song else {
            return;
        };
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_add_tracks(&client, pid, &[song])
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        self.ops.picker_song = None;
        cx.notify();
        // 待接口返回后再刷新列表
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.services.toast_success("已添加到歌单", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                } else {
                    this.services.toast_error("添加到歌单失败", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 新建歌单浮层。
    pub(crate) fn create_overlay(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak.clone();
        overlay_scrim_focusing(
            "create-overlay",
            weak,
            Root::close_create,
            self.ops.create_input.read(cx).focus_handle(cx),
            // 新建歌单：每次打开清空上次残留（重命名/简介要保留原值，故只有这里传 Some）。
            Some(self.ops.create_input.clone()),
        )
        .child(
            div()
                    .v_flex()
                    .w(px(360.0))
                    .gap(px(14.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(20.0))
                    // 屏蔽面板内点击冒泡到遮罩（点遮罩才关闭）。
                    .id("create-panel")
                    .on_click(move |_ev, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child("新建歌单"),
                    )
                    .child(Input::new(&self.ops.create_input))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(spacing::SM))
                            .child(
                                Button::new("create-cancel").label("取消").on_click(
                                    move |_ev, window, cx| {
                                        weak_cancel
                                            .update(cx, |root, cx| root.close_create(window, cx))
                                            .ok();
                                    },
                                ),
                            )
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
    pub(crate) fn picker_overlay(
        &self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak_close = weak.clone();
        let weak_create = weak.clone();
        let mut list = div().v_flex().w_full().gap(px(spacing::XS));
        for pl in &self.ops.picker_playlists {
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
                    .rounded(px(theme::RADIUS_SM))
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
        if self.ops.picker_playlists.is_empty() && !self.ops.picker_more {
            list = list.child(
                div()
                    .w_full()
                    .py(px(spacing::XL))
                    .flex()
                    .justify_center()
                    .child(div().text_sm().text_color(theme::text_gray()).child("暂无歌单")),
            );
        }
        if self.ops.picker_more {
            let weak_more = weak.clone();
            list = list.child(
                div().w_full().flex().items_center().justify_center().child(
                    Button::new("picker-more")
                        .ghost()
                        .label("加载更多")
                        .on_click(move |_ev, _w, cx| {
                            weak_more.update(cx, |root, cx| root.load_more_picker(cx)).ok();
                        }),
                ),
            );
        }
        overlay_scrim("picker-overlay", weak, Root::close_picker)
            .child(
                div()
                    .v_flex()
                    .w(px(360.0))
                    .h(px(500.0))
                    .gap(px(10.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(spacing::MD))
                    .id("picker-panel")
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
                                    .child("添加到歌单"),
                            )
                            .child(Button::new("picker-create").ghost().label("新建歌单").on_click(
                                move |_ev, window, cx| {
                                    weak_create
                                        .update(cx, |root, cx| {
                                            root.close_picker(window, cx);
                                            root.open_create(cx);
                                        })
                                        .ok();
                                },
                            )),
                    )
                    .child(smooth_scroll("picker-scroll", list, window, cx))
                    .child(
                        Button::new("picker-cancel")
                            .label("取消")
                            .on_click(move |_ev, window, cx| {
                                weak_close.update(cx, |root, cx| root.close_picker(window, cx)).ok();
                            }),
                    ),
            )
            .into_any_element()
    }

    /// 打开重命名歌单浮层。
    pub(crate) fn open_rename(&mut self, id: i64, cx: &mut Context<Self>) {
        overlay_focus("rename-overlay", cx);
        self.ops.rename_pid = Some(id);
        cx.notify();
    }

    pub(crate) fn close_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.rename_pid = None;
        overlay_close("rename-overlay", window, cx);
        cx.notify();
    }

    /// 提交重命名。
    pub(crate) fn submit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.ops.rename_pid else { return };
        let name = self.ops.rename_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.services.toast_error("歌单名不能为空", cx);
            return;
        }
        let client = self.services.client.clone();
        let new_name = name.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_rename(&client, id, &name)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        self.ops.rename_pid = None;
        cx.notify();
        // 待接口返回后再刷新列表，并即时更新当前打开歌单页的标题
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.services.toast_success("已重命名歌单", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                    this.update_open_playlist_name(id, &new_name, cx);
                } else {
                    this.services.toast_error("重命名失败", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 打开删除确认浮层（标题点名歌单）。
    pub(crate) fn open_delete(&mut self, id: i64, name: String, cx: &mut Context<Self>) {
        overlay_focus("delete-overlay", cx);
        self.ops.delete_pid = Some(id);
        DELETE_PNAME.with(|slot| *slot.borrow_mut() = Some(name));
        cx.notify();
    }

    pub(crate) fn close_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.delete_pid = None;
        DELETE_PNAME.with(|slot| *slot.borrow_mut() = None);
        overlay_close("delete-overlay", window, cx);
        cx.notify();
    }

    pub(crate) fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.ops.delete_pid else { return };
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_delete(&client, id).await.is_ok();
            let _ = tx.send(ok);
        });
        self.ops.delete_pid = None;
        DELETE_PNAME.with(|slot| *slot.borrow_mut() = None);
        cx.notify();
        // 待接口返回后再刷新列表；若删除的是当前打开的歌单则返回首页
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.services.toast_success("歌单已删除", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                    if matches!(this.route(), Route::Playlist { id: cur, .. } if cur == id) {
                        this.navigate(Route::Home, cx);
                    }
                } else {
                    this.services.toast_error("删除歌单失败", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 若当前打开的歌单正是该 id，更新其标题（导航栈与页内）。
    fn update_open_playlist_name(&mut self, id: i64, name: &str, cx: &mut Context<Self>) {
        if let Some(Route::Playlist { id: cur, name: n }) = self.nav.last_mut() {
            if *cur == id {
                *n = name.to_string();
            }
        }
        if let Some(page) = self.playlist_page.clone() {
            page.update(cx, |p, cx| p.rename_done(name.to_string(), cx));
        }
    }

    /// 重命名浮层。
    pub(crate) fn rename_overlay(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak.clone();
        overlay_scrim_focusing(
            "rename-overlay",
            weak,
            Root::close_rename,
            self.ops.rename_input.read(cx).focus_handle(cx),
            None,
        )
        .child(
            div()
                .v_flex()
                .w(px(360.0))
                .gap(px(14.0))
                .rounded(px(theme::RADIUS_LG))
                .bg(theme::surface())
                .p(px(20.0))
                .id("rename-panel")
                .on_click(move |_ev, _window, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("重命名歌单"),
                )
                .child(Input::new(&self.ops.rename_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(spacing::SM))
                        .child(
                            Button::new("rename-cancel")
                                .label("取消")
                                .on_click(move |_ev, window, cx| {
                                    weak_cancel.update(cx, |root, cx| root.close_rename(window, cx)).ok();
                                }),
                        )
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
        let weak_ok = weak.clone();
        let title = format!(
            "删除“{}”？",
            DELETE_PNAME.with(|slot| slot.borrow().clone()).unwrap_or_default()
        );
        overlay_scrim("delete-overlay", weak, Root::close_delete)
            .child(
                div()
                    .v_flex()
                    .w(px(340.0))
                    .gap(px(14.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(20.0))
                    .id("delete-panel")
                    .on_click(move |_ev, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child(title),
                    )
                    .child(div().text_sm().text_color(theme::text_gray()).child("此操作不可撤销。"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(spacing::SM))
                            .child(
                                Button::new("delete-cancel")
                                    .label("取消")
                                    .on_click(move |_ev, window, cx| {
                                        weak_cancel.update(cx, |root, cx| root.close_delete(window, cx)).ok();
                                    }),
                            )
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

    /// 打开编辑简介浮层。
    pub(crate) fn open_desc(&mut self, id: i64, cx: &mut Context<Self>) {
        overlay_focus("desc-overlay", cx);
        self.ops.desc_pid = Some(id);
        cx.notify();
    }

    pub(crate) fn close_desc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.desc_pid = None;
        overlay_close("desc-overlay", window, cx);
        cx.notify();
    }

    /// 提交编辑简介。
    pub(crate) fn submit_desc(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.ops.desc_pid else { return };
        let desc = self.ops.desc_input.read(cx).value().to_string();
        let client = self.services.client.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let ok = siren_core::network::api::playlist_update_desc(&client, id, &desc)
                .await
                .is_ok();
            let _ = tx.send(ok);
        });
        self.ops.desc_pid = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            this.update(cx, |this, cx| {
                if ok {
                    this.services.toast_success("简介已更新", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                } else {
                    this.services.toast_error("简介更新失败", cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 编辑简介浮层。
    pub(crate) fn desc_overlay(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let weak_cancel = weak.clone();
        let weak_ok = weak.clone();
        overlay_scrim_focusing(
            "desc-overlay",
            weak,
            Root::close_desc,
            self.ops.desc_input.read(cx).focus_handle(cx),
            None,
        )
        .child(
            div()
                .v_flex()
                .w(px(420.0))
                .gap(px(14.0))
                .rounded(px(theme::RADIUS_LG))
                .bg(theme::surface())
                .p(px(20.0))
                .id("desc-panel")
                .on_click(move |_ev, _window, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("编辑简介"),
                )
                .child(Input::new(&self.ops.desc_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(spacing::SM))
                        .child(
                            Button::new("desc-cancel")
                                .label("取消")
                                .on_click(move |_ev, window, cx| {
                                    weak_cancel.update(cx, |root, cx| root.close_desc(window, cx)).ok();
                                }),
                        )
                        .child(
                            Button::new("desc-ok")
                                .primary()
                                .label("保存")
                                .on_click(move |_ev, _w, cx| {
                                    weak_ok.update(cx, |root, cx| root.submit_desc(cx)).ok();
                                }),
                        ),
                ),
        )
        .into_any_element()
    }
}
