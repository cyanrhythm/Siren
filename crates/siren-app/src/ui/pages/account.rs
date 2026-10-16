//! 账号设置：等级 / VIP / 绑定 / 每日签到 / 改资料 / 改头像。
//!
//! 对齐 Melodia `feature/settings`（`SettingsApi` + `SettingsViewModel`）。

use gpui_kit::base::StyledExt;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::{AccountBinding, UserLevel, VipInfo};

use super::super::services::Services;
use super::super::session::Session;
use super::super::{fmt_date, page_shell, PageEvent};
use crate::theme::{self, spacing};

pub(crate) struct AccountPage {
    services: Services,
    nickname_input: Entity<InputState>,
    signature_input: Entity<InputState>,
    level: Option<UserLevel>,
    vip: Option<VipInfo>,
    bindings: Option<Vec<AccountBinding>>,
    loaded: bool,
    gen: u64,
}

impl EventEmitter<PageEvent> for AccountPage {}

impl AccountPage {
    pub(crate) fn new(
        cx: &mut Context<Self>,
        services: Services,
        nickname_input: Entity<InputState>,
        signature_input: Entity<InputState>,
    ) -> Self {
        let mut page = Self {
            services,
            nickname_input,
            signature_input,
            level: None,
            vip: None,
            bindings: None,
            loaded: false,
            gen: 0,
        };
        // 头像按 80px（bucket 128）渲染，需先预取。
        let avatar = Session::account(cx).map(|a| a.avatar_url.clone()).unwrap_or_default();
        if !avatar.is_empty() {
            page.services.covers.prefetch([avatar], 80.0, cx);
        }
        page.load(cx);
        page
    }

    /// 拉取等级 / VIP / 绑定信息。
    fn load(&mut self, cx: &mut Context<Self>) {
        self.gen = self.gen.wrapping_add(1);
        let gen = self.gen;
        let client = self.services.client.clone();
        let uid = Session::uid(cx).unwrap_or_default();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let level = siren_core::network::api::user_level(&client).await.ok();
            let vip = siren_core::network::api::vip_info(&client).await.ok();
            let bindings = siren_core::network::api::user_bindings(&client, uid).await.ok();
            let _ = tx.send((level, vip, bindings));
        });
        cx.spawn(async move |this, cx| {
            let payload = rx.await.ok();
            this.update(cx, |this, cx| {
                if this.gen != gen {
                    return;
                }
                if let Some((level, vip, bindings)) = payload {
                    if level.is_some() {
                        this.level = level;
                    }
                    if vip.is_some() {
                        this.vip = vip;
                    }
                    this.bindings = bindings;
                    this.loaded = true;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 每日签到。
    fn signin(&mut self, cx: &mut Context<Self>) {
        let client = self.services.client.clone();
        let services = self.services.clone();
        // reqwest 必须在 tokio 运行时上轮询：走 handle.spawn + oneshot，
        // 不能在 GPUI 前台执行器（主线程）里直接 .await（否则 "no reactor running" panic）。
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::daily_signin(&client, 0).await);
        });
        cx.spawn(async move |this, cx| {
            let res = rx.await.ok();
            this.update(cx, |this, cx| {
                match res {
                    Some(Ok(points)) if points > 0 => services.toast_success(format!("签到成功！积分+{points}"), cx),
                    Some(Ok(_)) => services.toast("今天已经签到过了", cx),
                    Some(Err(e)) => services.toast_error(e.user_message(), cx),
                    None => {}
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    /// 保存资料（昵称 + 个性签名；留空则保持原值）。
    fn save_profile(&mut self, cx: &mut Context<Self>) {
        let Some(acc) = Session::account(cx).cloned() else {
            return;
        };
        let typed_nick = self.nickname_input.read(cx).value().trim().to_string();
        let signature = self.signature_input.read(cx).value().trim().to_string();
        let nickname = if typed_nick.is_empty() {
            acc.nickname.to_string()
        } else {
            typed_nick
        };
        let client = self.services.client.clone();
        let services = self.services.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let nick_for_save = nickname.clone();
        self.services.handle.spawn(async move {
            let _ = tx.send(siren_core::network::api::update_user_profile(&client, &nick_for_save, &signature).await);
        });
        cx.spawn(async move |this, cx| {
            let res = rx.await.ok();
            this.update(cx, |_this, cx| match res {
                Some(Ok(())) => {
                    if let Some(mut a) = Session::account(cx).cloned() {
                        a.nickname = nickname.clone().into();
                        Session::set(cx, Some(a));
                    }
                    services.toast_success("资料保存成功", cx);
                }
                Some(Err(e)) => services.toast_error(e.user_message(), cx),
                None => {}
            })
            .ok();
        })
        .detach();
    }

    /// 更换头像：选图 → 居中裁方 → 压缩 JPEG → 上传。
    fn change_avatar(&mut self, cx: &mut Context<Self>) {
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择头像图片".into()),
        };
        let task = cx.prompt_for_paths(options);
        let client = self.services.client.clone();
        let services = self.services.clone();
        let handle = self.services.handle.clone();
        cx.spawn(async move |this, cx| {
            let picked = match task.await {
                Ok(Ok(paths)) => paths.and_then(|mut v| v.pop()),
                _ => None,
            };
            let Some(path) = picked else { return };
            let Some(jpeg) = std::fs::read(&path).ok().and_then(|b| square_jpeg(&b)) else {
                let msg = services.clone();
                this.update(cx, |_this, cx| msg.toast_error("无法读取该图片", cx)).ok();
                return;
            };
            // 上传（reqwest）放到 tokio 运行时，避免主线程轮询 panic。
            let (tx, rx) = tokio::sync::oneshot::channel();
            handle.spawn(async move {
                let _ = tx.send(siren_core::network::api::upload_avatar(&client, jpeg, "avatar.jpg").await);
            });
            let res = rx.await.ok();
            this.update(cx, |_this, cx| match res {
                Some(Ok(url)) if !url.is_empty() => {
                    if let Some(mut a) = Session::account(cx).cloned() {
                        a.avatar_url = url;
                        Session::set(cx, Some(a));
                    }
                    services.toast_success("头像更新成功", cx);
                }
                Some(Ok(_)) => services.toast_error("头像更新失败", cx),
                Some(Err(e)) => services.toast_error(e.user_message(), cx),
                None => {}
            })
            .ok();
        })
        .detach();
    }
}

impl Render for AccountPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let acc = Session::account(cx).cloned();
        // 头像异步可得（Session 可能晚于本页创建），命中缓存前每次渲染幂等地预取。
        if let Some(a) = &acc {
            if !a.avatar_url.is_empty() {
                self.services.covers.prefetch([a.avatar_url.clone()], 80.0, cx);
            }
        }
        let weak = cx.weak_entity();

        // ── 账号头部 ──
        let account_card = match &acc {
            Some(a) => {
                let weak_avatar = weak.clone();
                let avatar = div()
                    .id("account-avatar")
                    .size(px(80.0))
                    .flex_none()
                    .rounded_full()
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(move |_ev, _window, cx| {
                        weak_avatar.update(cx, |page, cx| page.change_avatar(cx)).ok();
                    })
                    .child(self.services.covers.image_round(&a.avatar_url, 80.0));
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(
                        div().flex().items_center().gap(px(spacing::LG)).child(avatar).child(
                            div()
                                .v_flex()
                                .gap(px(spacing::XS))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme::text_primary())
                                        .child(a.nickname.to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_gray())
                                        .child(format!("UID {}", a.uid)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_gray())
                                        .child("点击头像可更换".to_string()),
                                ),
                        ),
                    )
                    .into_any_element()
            }
            None => div()
                .text_sm()
                .text_color(theme::text_gray())
                .child("未登录")
                .into_any_element(),
        };

        // ── 等级 ──
        let level_card = match &self.level {
            Some(l) => {
                let now = l.now_play_count;
                let next = l.next_play_count;
                let p = normalize_progress(l.progress, now as f64, next as f64);
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(spacing::SM))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(spacing::SM))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child(format!("Lv.{}", l.level)),
                            )
                            .child(div().text_xs().text_color(theme::text_gray()).child(format!(
                                "听歌 {now}/{next} · 登录 {}/{}",
                                l.now_login_count, l.next_login_count
                            ))),
                    )
                    .child(progress_bar(p))
                    .into_any_element()
            }
            None => div()
                .text_sm()
                .text_color(theme::text_gray())
                .child(if self.loaded {
                    "等级信息不可用"
                } else {
                    "加载中…"
                })
                .into_any_element(),
        };

        // ── VIP ──
        let vip_card = match &self.vip {
            Some(v) => {
                let label = match v.vip_type {
                    11 => "黑胶 VIP",
                    10 => "音乐包",
                    _ if v.is_vip => "VIP",
                    _ => "普通用户",
                };
                let expire = if v.expire_time > 0 {
                    format!("有效期至 {}", fmt_date(v.expire_time))
                } else {
                    String::new()
                };
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(spacing::XS))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme::text_primary())
                            .child(label),
                    )
                    .when(!expire.is_empty(), |s| {
                        s.child(div().text_xs().text_color(theme::text_gray()).child(expire))
                    })
                    .into_any_element()
            }
            None => div()
                .text_sm()
                .text_color(theme::text_gray())
                .child(if self.loaded { "普通用户" } else { "加载中…" })
                .into_any_element(),
        };

        // ── 账号绑定 ──
        let mut bindings_col = div().v_flex().w_full().gap(px(spacing::XXS));
        match &self.bindings {
            None => {
                bindings_col =
                    bindings_col.child(div().text_sm().text_color(theme::text_gray()).child(if self.loaded {
                        "绑定信息不可用"
                    } else {
                        "加载中…"
                    }));
            }
            Some(list) if list.is_empty() => {
                bindings_col = bindings_col.child(div().text_sm().text_color(theme::text_gray()).child("暂无绑定信息"));
            }
            Some(list) => {
                for b in list {
                    let status = if b.expired { "已过期" } else { "已绑定" };
                    bindings_col = bindings_col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme::text_primary())
                                    .child(b.type_name.clone()),
                            )
                            .child(div().text_xs().text_color(theme::text_gray()).child(status)),
                    );
                }
            }
        }

        // ── 每日签到 ──
        let weak_signin = weak.clone();
        let signin_card = div()
            .id("account-signin")
            .px(px(spacing::LG))
            .py(px(spacing::SM))
            .rounded_full()
            .cursor_pointer()
            .bg(theme::accent())
            .hover(|s| s.opacity(0.85))
            .on_click(move |_ev, _window, cx| {
                weak_signin.update(cx, |page, cx| page.signin(cx)).ok();
            })
            .child(div().text_sm().text_color(theme::text_primary()).child("每日签到"));

        // ── 改资料 ──
        let weak_save = weak.clone();
        let edit_card = div()
            .v_flex()
            .w_full()
            .gap(px(10.0))
            .child(
                div()
                    .v_flex()
                    .gap(px(spacing::XS))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_gray())
                            .child("新昵称（留空保持不变）"),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .px(px(10.0))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(theme::pane())
                            .child(Input::new(&self.nickname_input)),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap(px(spacing::XS))
                    .child(div().text_xs().text_color(theme::text_gray()).child("个性签名"))
                    .child(
                        div()
                            .w_full()
                            .h(px(36.0))
                            .flex()
                            .items_center()
                            .px(px(10.0))
                            .rounded(px(theme::RADIUS_SM))
                            .bg(theme::pane())
                            .child(Input::new(&self.signature_input)),
                    ),
            )
            .child(
                div()
                    .id("account-save")
                    .px(px(spacing::LG))
                    .py(px(spacing::SM))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(theme::surface())
                    .hover(|s| s.bg(theme::surface_light()))
                    .on_click(move |_ev, _window, cx| {
                        weak_save.update(cx, |page, cx| page.save_profile(cx)).ok();
                    })
                    .child(div().text_sm().text_color(theme::text_primary()).child("保存资料")),
            );

        let body = div()
            .v_flex()
            .flex_1()
            .min_w(px(0.0))
            .w_full()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(card("账号", account_card))
            .child(card("等级", level_card))
            .child(card("会员", vip_card))
            .child(card("账号绑定", bindings_col.into_any_element()))
            .child(card("签到", signin_card.into_any_element()))
            .child(card("资料", edit_card.into_any_element()))
            .into_any_element();

        page_shell("账号设置".to_string(), body)
    }
}

/// 设置分节卡片：标题 + 内容。
fn card(title: &str, content: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(theme::card_surface())
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(content)
        .into_any_element()
}

fn progress_bar(p: f32) -> AnyElement {
    div()
        .w_full()
        .h(px(6.0))
        .rounded_full()
        .bg(theme::pane())
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme::accent())
                .w(relative(p.clamp(0.0, 1.0))),
        )
        .into_any_element()
}

/// 归一化进度：优先用接口 `progress`（可能为百分比），异常时用听歌数推算。
fn normalize_progress(progress: f64, now: f64, next: f64) -> f32 {
    let p = if progress > 1.0 { progress / 100.0 } else { progress };
    if p > 0.0 && p.is_finite() {
        p as f32
    } else if next > 0.0 {
        (now / next) as f32
    } else {
        0.0
    }
}

/// 居中裁成正方形并压缩为 JPEG（最长边 512）。
fn square_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    let side = w.min(h);
    if side == 0 {
        return None;
    }
    let cropped = img.crop_imm((w - side) / 2, (h - side) / 2, side, side);
    let cropped = if side > 512 {
        cropped.resize(512, 512, image::imageops::FilterType::Lanczos3)
    } else {
        cropped
    };
    let rgb = cropped.to_rgb8();
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
    enc.encode_image(&rgb).ok()?;
    Some(buf)
}
