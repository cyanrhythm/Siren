//! 歌单写操作浮层：新建歌单、收藏歌曲到歌单。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use gpui_kit::base::{ElementExt as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::services::task_failure;
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

/// 浮层里字段上方的说明文字（灰、小号）。
fn field_label(text: String) -> impl IntoElement {
    div().text_xs().text_color(theme::text_gray()).child(text)
}

/// 同 [`overlay_scrim`]，但打开后把焦点直接落到浮层内的输入框（`inner`），
/// 用户打开即可输入，无需先点一下输入框（整改清单 O0.2）。
///
/// `reset_inputs` 里的输入框会在打开的首帧被清空——避免上次没提交就关掉时
/// 残留的文字带到下一次打开（如「新建歌单」上一次没提交的名字）。
pub(crate) fn overlay_scrim_focusing<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
    inner: FocusHandle,
    reset_inputs: Vec<Entity<InputState>>,
) -> Stateful<Div> {
    overlay_scrim_inner(id, weak, close, theme::scrim(), Some(inner), reset_inputs)
}

fn overlay_scrim_with<E: 'static>(
    id: &'static str,
    weak: WeakEntity<E>,
    close: impl Fn(&mut E, &mut Window, &mut Context<E>) + Clone + 'static,
    scrim: Rgba,
) -> Stateful<Div> {
    overlay_scrim_inner(id, weak, close, scrim, None, Vec::new())
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
    reset_inputs: Vec<Entity<InputState>>,
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
                for input in reset_inputs.clone() {
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
            let _ = tx.send(siren_core::network::api::playlist_create(&client, &name, 0).await);
        });
        self.ops.create_open = false;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let failure = task_failure(&result, "创建歌单");
            this.update(cx, |this, cx| {
                if let Some(msg) = failure {
                    this.services.toast_error(msg, cx);
                } else {
                    this.services.toast_success("歌单已创建", cx);
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
                    None => {
                        log::warn!("加载歌单失败：任务未返回（通道断开）");
                        this.services.toast_error("加载歌单失败，请重试", cx)
                    }
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
                    None => {
                        log::warn!("加载歌单失败：任务未返回（通道断开）");
                        this.services.toast_error("加载歌单失败，请重试", cx)
                    }
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
            let _ = tx.send(siren_core::network::api::playlist_add_tracks(&client, pid, &[song]).await);
        });
        self.ops.picker_song = None;
        cx.notify();
        // 待接口返回后再刷新列表
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let failure = task_failure(&result, "添加到歌单");
            this.update(cx, |this, cx| {
                if let Some(msg) = failure {
                    this.services.toast_error(msg, cx);
                } else {
                    this.services.toast_success("已添加到歌单", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
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
            // 每次打开清空上次残留（未提交就关掉的文字不带到下一次）。
            vec![self.ops.create_input.clone()],
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

    /// 打开「名称和详情」对话框（名称 / 简介 / 封面三合一）。
    ///
    /// `name` / `description` / `cover_url` 是**当前值**，只用于在对话框里展示：
    /// kit 的 `InputState` 没有公开的写入接口（`replace_all` 是 crate 私有），
    /// 所以输入框不预填，改为「留空 = 该项不修改」——同时也避免了旧版
    /// 「编辑简介打开是空框、保存就把简介清空」的隐患。
    pub(crate) fn open_edit(
        &mut self,
        id: i64,
        name: String,
        description: String,
        cover_url: String,
        cx: &mut Context<Self>,
    ) {
        overlay_focus("edit-overlay", cx);
        self.ops.edit = Some((id, name, description, cover_url));
        cx.notify();
    }

    pub(crate) fn close_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ops.edit = None;
        overlay_close("edit-overlay", window, cx);
        cx.notify();
    }

    /// 「更换封面」：先关闭对话框再进入选图 → 裁剪 → 上传流程。
    ///
    /// 裁剪浮层属于页面元素树，若与对话框同开会被对话框压住，故先关窗。
    fn change_edit_cover(&mut self, cx: &mut Context<Self>) {
        self.ops.edit = None;
        cx.notify();
        if let Some(page) = self.playlist_page.clone() {
            page.update(cx, |page, cx| page.change_cover(cx));
        }
    }

    /// 提交「名称和详情」：只提交非空字段（留空 = 该项不修改）。
    pub(crate) fn submit_edit(&mut self, cx: &mut Context<Self>) {
        let Some((id, _cur_name, _cur_desc, _)) = self.ops.edit.clone() else {
            return;
        };
        let name = self.ops.edit_name_input.read(cx).value().trim().to_string();
        let desc = self.ops.edit_desc_input.read(cx).value().trim().to_string();
        if name.is_empty() && desc.is_empty() {
            self.services.toast_error("名称与简介均为空，没有可保存的修改", cx);
            return;
        }
        let client = self.services.client.clone();
        let new_name = name.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            // 两项都是「非空才提交」，任一失败即整体失败：保留首个错误供提示。
            let result: siren_core::Result<()> = async {
                if !name.is_empty() {
                    siren_core::network::api::playlist_rename(&client, id, &name).await?;
                }
                if !desc.is_empty() {
                    siren_core::network::api::playlist_update_desc(&client, id, &desc).await?;
                }
                Ok(())
            }
            .await;
            let _ = tx.send(result);
        });
        self.ops.edit = None;
        cx.notify();
        // 待接口返回后再刷新列表，并即时更新当前打开歌单页的标题。
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let failure = task_failure(&result, "保存歌单信息");
            this.update(cx, |this, cx| {
                if let Some(msg) = failure {
                    this.services.toast_error(msg, cx);
                } else {
                    this.services.toast_success("已保存歌单信息", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                    if !new_name.is_empty() {
                        this.update_open_playlist_name(id, &new_name, cx);
                    }
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
            let _ = tx.send(siren_core::network::api::playlist_delete(&client, id).await);
        });
        self.ops.delete_pid = None;
        DELETE_PNAME.with(|slot| *slot.borrow_mut() = None);
        cx.notify();
        // 待接口返回后再刷新列表；若删除的是当前打开的歌单则返回首页
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let failure = task_failure(&result, "删除歌单");
            this.update(cx, |this, cx| {
                if let Some(msg) = failure {
                    this.services.toast_error(msg, cx);
                } else {
                    this.services.toast_success("歌单已删除", cx);
                    this.library.update(cx, |page, cx| page.reload_playlists(cx));
                    if matches!(this.route(), Route::Playlist { id: cur, .. } if cur == id) {
                        this.navigate(Route::Home, cx);
                    }
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

    /// 「名称和详情」对话框：名称 + 简介 + 封面（对齐 Melodia `EditPlaylistDialog`）。
    pub(crate) fn edit_overlay(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let Some((_id, name, desc, cover_url)) = self.ops.edit.clone() else {
            return div().into_any_element();
        };
        let weak_cancel = weak.clone();
        let weak_ok = weak.clone();
        let weak_cover = weak;
        let cover = (!cover_url.is_empty()).then(|| self.services.covers.image(&cover_url, 72.0));
        let cur_desc = if desc.trim().is_empty() {
            "（暂无简介）".to_string()
        } else {
            desc.clone()
        };
        overlay_scrim_focusing(
            "edit-overlay",
            weak_cancel.clone(),
            Root::close_edit,
            self.ops.edit_name_input.read(cx).focus_handle(cx),
            vec![self.ops.edit_name_input.clone(), self.ops.edit_desc_input.clone()],
        )
        .child(
            div()
                .v_flex()
                .w(px(430.0))
                .gap(px(spacing::MD))
                .rounded(px(theme::RADIUS_LG))
                .bg(theme::surface())
                .p(px(20.0))
                .id("edit-panel")
                .on_click(move |_ev, _window, cx| cx.stop_propagation())
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text_primary())
                        .child("名称和详情"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(spacing::MD))
                        .when_some(cover, |row, cover| row.child(cover))
                        .child(
                            Button::new("edit-cover")
                                .label("更换封面")
                                .on_click(move |_ev, _w, cx| {
                                    weak_cover.update(cx, |root, cx| root.change_edit_cover(cx)).ok();
                                }),
                        ),
                )
                .child(field_label(format!("当前名称：{name}")))
                .child(Input::new(&self.ops.edit_name_input))
                .child(field_label(format!("当前简介：{cur_desc}")))
                .child(Input::new(&self.ops.edit_desc_input))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(spacing::SM))
                        .child(
                            Button::new("edit-cancel")
                                .label("取消")
                                .on_click(move |_ev, window, cx| {
                                    weak_cancel.update(cx, |root, cx| root.close_edit(window, cx)).ok();
                                }),
                        )
                        .child(
                            Button::new("edit-ok")
                                .primary()
                                .label("保存")
                                .on_click(move |_ev, _w, cx| {
                                    weak_ok.update(cx, |root, cx| root.submit_edit(cx)).ok();
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
}
