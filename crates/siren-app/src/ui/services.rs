//! 共享服务：跨页面复用的后端能力（网络客户端、运行时句柄、封面缓存、全局提示）。
//!
//! 各页面持有其克隆，依赖共享服务而非外壳，符合 coding-guides 的依赖方向。

use std::sync::Arc;

use gpui_kit::*;

use siren_core::network::client::NeteaseClient;

use super::covers::CoverLoader;
use super::notifications::Notifications;

#[derive(Clone)]
pub(crate) struct Services {
    pub client: Arc<NeteaseClient>,
    pub handle: tokio::runtime::Handle,
    pub covers: CoverLoader,
    /// 全局提示（Toast）唯一真源。
    pub notifications: Entity<Notifications>,
}

impl Services {
    /// 失败提示。
    pub(crate) fn toast_error(&self, text: impl Into<SharedString>, cx: &mut App) {
        self.notifications.update(cx, |n, cx| n.error(text, cx));
    }

    /// 成功提示。
    pub(crate) fn toast_success(&self, text: impl Into<SharedString>, cx: &mut App) {
        self.notifications.update(cx, |n, cx| n.success(text, cx));
    }

    /// 普通提示。
    pub(crate) fn toast(&self, text: impl Into<SharedString>, cx: &mut App) {
        self.notifications.update(cx, |n, cx| n.info(text, cx));
    }
}
