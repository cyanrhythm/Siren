//! 登录弹窗：扫码 / 网页 / Cookie 三种方式。
//!
//! 对齐 Melodia 桌面端 `LoginDialog.kt`。二维码由 siren-core 生成 PNG，经 gpui 解码渲染。

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{icon, Root};
use crate::theme::{self, spacing};

thread_local! {
    /// 登录浮层的焦点句柄：懒建后稳定复用，track_focus 依赖它把按键送达浮层根。
    static LOGIN_OVERLAY_FOCUS: RefCell<Option<FocusHandle>> = const { RefCell::new(None) };
}

/// 取（或建）登录浮层的焦点句柄。
fn overlay_focus(cx: &mut App) -> FocusHandle {
    LOGIN_OVERLAY_FOCUS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(cx.focus_handle());
        }
        slot.as_ref().unwrap().clone()
    })
}

/// 浮层遮罩根：全屏压暗，点遮罩 / Esc 关闭；焦点为空时兜底聚焦浮层根（Esc 才能送达）。
fn overlay_scrim(weak: WeakEntity<Root>) -> Stateful<Div> {
    let weak_click = weak.clone();
    let weak_esc = weak;
    let close = Root::close_login;
    let mut root = div()
        .id("login-overlay")
        .occlude()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::scrim())
        .on_click(move |ev: &ClickEvent, window, cx| {
            // 只响应鼠标左键；浮层根聚焦时 Enter/Space 产生的键盘 click 不关浮层。
            let on_scrim = matches!(ev, ClickEvent::Mouse(e) if e.up.button == MouseButton::Left);
            if on_scrim {
                weak_click.update(cx, |root, cx| close(root, window, cx)).ok();
            }
        })
        .on_key_down(move |ev: &KeyDownEvent, window, cx| {
            if ev.keystroke.key == "escape" {
                weak_esc.update(cx, |root, cx| close(root, window, cx)).ok();
                cx.stop_propagation();
            }
        })
        .on_prepaint(move |_, window, cx| {
            // 打开后若无焦点（点按钮不聚焦），兜底聚焦浮层根，Esc 才能送达。
            let focus = overlay_focus(cx);
            if window.focused(cx).is_none() {
                window.focus(&focus, cx);
            }
        });
    if let Some(focus) = LOGIN_OVERLAY_FOCUS.with(|slot| slot.borrow().clone()) {
        root = root.track_focus(&focus);
    }
    root
}

/// 登录方式标签。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginTab {
    Scan,
    Web,
    Cookie,
}

impl LoginTab {
    fn label(self) -> &'static str {
        match self {
            LoginTab::Scan => "扫码",
            LoginTab::Web => "网页",
            LoginTab::Cookie => "Cookie",
        }
    }
}

impl Root {
    /// 打开登录弹窗（默认停在扫码）。
    pub(crate) fn open_login(&mut self, cx: &mut Context<Self>) {
        overlay_focus(cx);
        self.login.open = true;
        self.set_login_tab(LoginTab::Scan, cx);
    }

    /// 关闭登录弹窗并使进行中的二维码轮询失效。
    pub(crate) fn close_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.login.open = false;
        self.login.gen = self.login.gen.wrapping_add(1);
        window.blur(cx);
        cx.notify();
    }

    fn set_login_tab(&mut self, tab: LoginTab, cx: &mut Context<Self>) {
        self.login.tab = tab;
        self.login.gen = self.login.gen.wrapping_add(1);
        self.login.qr_image = None;
        if tab == LoginTab::Scan {
            self.login.status = "正在生成二维码…".into();
            self.start_qr(cx);
        } else {
            self.login.status = SharedString::default();
        }
        cx.notify();
    }

    /// Cookie 登录：校验并提交。
    pub(crate) fn submit_cookie_login(&mut self, cx: &mut Context<Self>) {
        let raw = self.login.input.read(cx).value().trim().to_string();
        let Some(cookie) = siren_core::auth::normalize_cookie(&raw) else {
            self.login.status = "请输入 MUSIC_U 或完整 Cookie".into();
            cx.notify();
            return;
        };
        self.finish_login(cookie, cx);
    }

    /// 登录成功：写入会话并让音乐库刷新账号（进而上报头像/昵称）。
    fn finish_login(&mut self, cookie: String, cx: &mut Context<Self>) {
        self.services.client.set_cookie(Some(cookie.clone()));
        siren_core::auth::save_cookie(&cookie);
        self.login.open = false;
        self.login.gen = self.login.gen.wrapping_add(1);
        cx.notify();
        self.library.update(cx, |page, cx| page.refresh_account(true, cx));
    }

    /// 生成二维码并轮询扫码状态。
    fn start_qr(&mut self, cx: &mut Context<Self>) {
        let gen = self.login.gen;
        let client = self.services.client.clone();
        let handle = self.services.handle.clone();
        cx.spawn(async move |this, cx| {
            // 取 unikey
            let (tx, rx) = tokio::sync::oneshot::channel();
            let client_key = client.clone();
            handle.spawn(async move {
                let key = siren_core::network::api::login_qr_key(&client_key)
                    .await
                    .ok()
                    .filter(|k| !k.is_empty());
                let _ = tx.send(key);
            });
            let Some(key) = rx.await.ok().flatten() else {
                this.update(cx, |this, cx| {
                    if this.login.gen == gen {
                        this.login.status = "二维码生成失败，请重试".into();
                        cx.notify();
                    }
                })
                .ok();
                return;
            };
            if this.update(cx, |this, _| this.login.gen != gen).unwrap_or(true) {
                return;
            }

            // 生成二维码图片（CPU 任务）
            let url = siren_core::network::api::qr_login_url(&key);
            let png = cx
                .background_executor()
                .spawn(async move { siren_core::auth::qr::qr_png(&url, 6, 2) })
                .await;
            let image = png.map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            this.update(cx, |this, cx| {
                if this.login.gen != gen {
                    return;
                }
                this.login.qr_image = image;
                this.login.status = "请使用网易云音乐 App 扫码".into();
                cx.notify();
            })
            .ok();

            // 轮询扫码状态
            let mut failures = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(2500)).await;
                if this.update(cx, |this, _| this.login.gen != gen).unwrap_or(true) {
                    return;
                }

                let (tx, rx) = tokio::sync::oneshot::channel();
                let client = client.clone();
                let key = key.clone();
                handle.spawn(async move {
                    let result = siren_core::network::api::login_qr_check(&client, &key).await;
                    let _ = tx.send(result);
                });
                let Some((code, cookie, message)) = rx.await.ok().and_then(|r| r.ok()) else {
                    failures += 1;
                    if failures >= 5 {
                        this.update(cx, |this, cx| {
                            if this.login.gen == gen {
                                this.login.status = "网络异常，请重试".into();
                                cx.notify();
                            }
                        })
                        .ok();
                        return;
                    }
                    continue;
                };
                failures = 0;

                match code {
                    800 => {
                        this.update(cx, |this, cx| {
                            if this.login.gen == gen {
                                this.login.status = "二维码已过期，请刷新".into();
                                cx.notify();
                            }
                        })
                        .ok();
                        return;
                    }
                    801 => self_status(&this, cx, gen, "等待扫码"),
                    802 => self_status(&this, cx, gen, "已扫码，请在手机上确认"),
                    803 => {
                        match cookie.filter(|c| !c.is_empty()) {
                            Some(cookie) => {
                                this.update(cx, |this, cx| this.finish_login(cookie, cx)).ok();
                            }
                            None => self_status(&this, cx, gen, "登录成功但未获取到会话，请重试"),
                        }
                        return;
                    }
                    _ => {
                        // 未知状态码多为中间态/临时异常：仅更新提示并继续轮询，
                        // 不要永久停止（二维码自身到期会走 800）。
                        let message = if message.is_empty() {
                            format!("扫码异常({code})")
                        } else {
                            message
                        };
                        this.update(cx, |this, cx| {
                            if this.login.gen == gen {
                                this.login.status = message.into();
                                cx.notify();
                            }
                        })
                        .ok();
                        continue;
                    }
                }
            }
        })
        .detach();
    }

    /// 登录弹窗浮层。
    pub(crate) fn login_overlay(&self, weak: WeakEntity<Self>) -> AnyElement {
        let tabs = div()
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .child(tab_button(LoginTab::Scan, self.login.tab, weak.clone()))
            .child(tab_button(LoginTab::Web, self.login.tab, weak.clone()))
            .child(tab_button(LoginTab::Cookie, self.login.tab, weak.clone()));

        let weak_cancel = weak.clone();
        let weak_cancel2 = weak_cancel.clone();
        let content: AnyElement = match self.login.tab {
            LoginTab::Scan => {
                let qr: AnyElement = match &self.login.qr_image {
                    Some(image) => img(image.clone()).w(px(210.0)).h(px(210.0)).into_any_element(),
                    None => div()
                        .text_sm()
                        .text_color(theme::text_gray())
                        .child("生成中…")
                        .into_any_element(),
                };
                let weak_retry = weak.clone();
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .size(px(226.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(theme::RADIUS_LG))
                            .bg(rgba(0xffffffff))
                            .child(qr),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child(self.login.status.clone()),
                    )
                    .child(
                        Button::new("qr-retry")
                            .label("刷新二维码")
                            .on_click(move |_ev, _window, cx| {
                                weak_retry
                                    .update(cx, |root, cx| root.set_login_tab(LoginTab::Scan, cx))
                                    .ok();
                            }),
                    )
                    .into_any_element()
            }
            LoginTab::Web => {
                let weak_open = weak.clone();
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child("在浏览器中完成网易云登录后，切到 Cookie 标签粘贴 Cookie"),
                    )
                    .child(Button::new("web-open").primary().label("打开官方登录页").on_click(
                        move |_ev, _window, _cx| {
                            siren_platform::open_url("https://music.163.com/#/login");
                            let _ = &weak_open;
                        },
                    ))
                    .into_any_element()
            }
            LoginTab::Cookie => {
                let weak_login = weak.clone();
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(spacing::MD))
                    .child(div().w(px(320.0)).child(Input::new(&self.login.input)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_gray())
                            .child(self.login.status.clone()),
                    )
                    .child(
                        Button::new("cookie-login")
                            .primary()
                            .label("登录")
                            .on_click(move |_ev, _window, cx| {
                                weak_login.update(cx, |root, cx| root.submit_cookie_login(cx)).ok();
                            }),
                    )
                    .into_any_element()
            }
        };

        // Enter 提交当前登录方式（Esc 关闭由 overlay_scrim 处理）。
        let weak_enter = weak.clone();
        let tab = self.login.tab;
        overlay_scrim(weak)
            .on_key_down(move |ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "enter" {
                    match tab {
                        LoginTab::Cookie => {
                            weak_enter.update(cx, |root, cx| root.submit_cookie_login(cx)).ok();
                        }
                        LoginTab::Web => {
                            siren_platform::open_url("https://music.163.com/#/login");
                        }
                        LoginTab::Scan => {}
                    }
                }
            })
            .child(
                div()
                    .w(px(380.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(14.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(theme::surface())
                    .p(px(spacing::XL))
                    .id("login-panel")
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
                                    .child("登录"),
                            )
                            .child(
                                Button::new("login-close")
                                    .ghost()
                                    .icon(icon(IconName::X, 16.0, theme::text_gray()))
                                    .tooltip("关闭")
                                    .on_click(move |_ev, window, cx| {
                                        weak_cancel.update(cx, |root, cx| root.close_login(window, cx)).ok();
                                    }),
                            ),
                    )
                    .child(tabs)
                    .child(content)
                    .child(
                        Button::new("login-cancel")
                            .label("取消")
                            .on_click(move |_ev, window, cx| {
                                weak_cancel2.update(cx, |root, cx| root.close_login(window, cx)).ok();
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// 更新扫码状态文案（仅当代数未变）。
fn self_status(this: &WeakEntity<Root>, cx: &mut AsyncApp, gen: u64, text: &str) {
    let text = text.to_string();
    this.update(cx, |this, cx| {
        if this.login.gen == gen {
            this.login.status = text.into();
            cx.notify();
        }
    })
    .ok();
}

/// 登录方式标签按钮。
fn tab_button(tab: LoginTab, current: LoginTab, weak: WeakEntity<Root>) -> impl IntoElement {
    let active = tab == current;
    div()
        .id(("login-tab", tab as usize))
        .px(px(14.0))
        .py(px(6.0))
        .rounded_full()
        .cursor_pointer()
        .bg(if active { theme::accent() } else { theme::pane() })
        .when(!active, |s| s.hover(|s| s.bg(theme::card_surface())))
        .on_click(move |_ev, _window, cx| {
            weak.update(cx, |root, cx| root.set_login_tab(tab, cx)).ok();
        })
        .child(
            div()
                .text_sm()
                .text_color(if active {
                    theme::text_primary()
                } else {
                    theme::text_gray()
                })
                .child(tab.label()),
        )
}
