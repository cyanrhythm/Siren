//! 全局提示（Toast）：窗口顶部居中（搜索栏下方）的轻量通知，替代散落的页面 `status` 文字。
//!
//! 设计对齐 Melodia 桌面端的 `SnackbarHost`：所有提示汇入单一通道，展示在播放条上方，
//! 定时自动消失。约定见 `docs/10-状态管理约定.md`。

use std::time::Duration;

use gpui_kit::*;

use super::Root;
use crate::theme;

/// 提示级别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Success,
    Error,
}

/// 一条提示。
pub(crate) struct Toast {
    id: u64,
    kind: ToastKind,
    text: SharedString,
}

/// 提示队列（唯一真源）。
pub(crate) struct Notifications {
    items: Vec<Toast>,
    next_id: u64,
}

/// 同时最多展示的提示数。
const MAX_VISIBLE: usize = 3;
/// 单条提示存活时长（毫秒）。
const TOAST_MS: u64 = 3600;

impl Notifications {
    pub(crate) fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            items: Vec::new(),
            next_id: 1,
        })
    }

    /// 当前待展示的提示（旧 → 新）。
    pub(crate) fn active(&self) -> &[Toast] {
        &self.items
    }

    fn push(&mut self, kind: ToastKind, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        // 同文案去重，避免连点叠加
        if self.items.iter().any(|t| t.text == text) {
            return;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.items.push(Toast { id, kind, text });
        while self.items.len() > MAX_VISIBLE {
            self.items.remove(0);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(TOAST_MS)).await;
            let _ = this.update(cx, |this, cx| this.remove(id, cx));
        })
        .detach();
    }

    fn remove(&mut self, id: u64, cx: &mut Context<Self>) {
        let before = self.items.len();
        self.items.retain(|t| t.id != id);
        if self.items.len() != before {
            cx.notify();
        }
    }

    /// 普通信息。
    pub(crate) fn info(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Info, text, cx);
    }

    /// 成功提示。
    pub(crate) fn success(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Success, text, cx);
    }

    /// 失败提示。
    pub(crate) fn error(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push(ToastKind::Error, text, cx);
    }
}

impl Root {
    /// 顶部居中的提示浮层（位于搜索栏下方）。
    pub(crate) fn notifications_overlay(&self, cx: &App) -> AnyElement {
        let items = self.services.notifications.read(cx).active();
        if items.is_empty() {
            return div().into_any_element();
        }
        let mut col = div().flex().flex_col().items_center().gap(px(8.0));
        for toast in items {
            col = col.child(toast_box(toast));
        }
        div()
            .absolute()
            .left_0()
            .right_0()
            .top(px(72.0))
            .flex()
            .justify_center()
            .child(col)
            .into_any_element()
    }
}

fn toast_box(toast: &Toast) -> AnyElement {
    let bar = match toast.kind {
        ToastKind::Error => theme::accent(),
        ToastKind::Success => theme::text_primary(),
        ToastKind::Info => theme::text_gray(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(8.0))
        .bg(theme::card_surface())
        .child(div().w(px(3.0)).h(px(16.0)).rounded_full().bg(bar))
        .child(
            div()
                .text_sm()
                .text_color(theme::text_primary())
                .child(toast.text.clone()),
        )
        .into_any_element()
}
