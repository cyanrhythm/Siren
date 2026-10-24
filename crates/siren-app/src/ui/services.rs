//! 共享服务：跨页面复用的后端能力（网络客户端、运行时句柄、封面缓存、全局提示）。
//!
//! 各页面持有其克隆，依赖共享服务而非外壳，符合 coding-guides 的依赖方向。

use std::sync::Arc;

use gpui_kit::*;

use siren_core::error::Error;
use siren_core::network::client::NeteaseClient;

use super::covers::CoverLoader;
use super::notifications::Notifications;

#[derive(Clone)]
pub(crate) struct Services {
    pub client: Arc<NeteaseClient>,
    pub handle: tokio::runtime::Handle,
    pub covers: CoverLoader,
    /// 元数据缓存（歌单/专辑/歌手 JSON 快照；取不到缓存目录时为 None）。
    pub meta: Option<crate::meta_cache::MetaCache>,
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

/// 命令类异步任务（订阅 / 关注 / 增删改）的统一失败判定。
///
/// 返回 `Some(文案)` 表示失败，调用方据此回滚乐观更新并 Toast：
/// - `Err(Error)` → [`Error::user_message`]，把「操作失败，请重试」换成可操作的原因
///   （「网络连接失败，请检查网络后重试」/「登录已失效，请重新登录」…）；
/// - 通道断开（任务 panic / 运行时关闭）→ 兜底文案，避免界面毫无反应。
///
/// 两种情况都记一条 `warn`，否则「界面提示重试、日志却没线索」，线上无从定位。
pub(crate) fn task_failure<T>(
    result: &std::result::Result<std::result::Result<T, Error>, tokio::sync::oneshot::error::RecvError>,
    context: &str,
) -> Option<&'static str> {
    match result {
        Ok(Ok(_)) => None,
        Ok(Err(err)) => {
            log::warn!("{context}失败：{err}");
            Some(err.user_message())
        }
        Err(err) => {
            log::warn!("{context}任务未返回（通道断开）：{err}");
            Some("操作未完成，请重试")
        }
    }
}

#[cfg(test)]
mod tests {
    // 不用 `use super::*`：它会把父模块的 `gpui_kit::*` 一并带进来，
    // 展开 `#[test]` 时撞上默认递归深度上限（见 lib.rs 的 `recursion_limit` 注释）。
    use super::task_failure;
    use siren_core::error::Error;

    type TaskResult<T> = std::result::Result<std::result::Result<T, Error>, tokio::sync::oneshot::error::RecvError>;

    /// 构造一个「发送端已销毁」的 `RecvError`。
    fn dropped_channel_error() -> tokio::sync::oneshot::error::RecvError {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        drop(tx);
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("建 tokio 运行时失败")
            .block_on(rx)
            .expect_err("发送端已销毁时必须返回 Err")
    }

    #[test]
    fn success_reports_no_failure() {
        let ok: TaskResult<()> = Ok(Ok(()));
        assert_eq!(task_failure(&ok, "测试"), None);
    }

    #[test]
    fn api_error_maps_to_user_message_not_generic_copy() {
        // 关键契约：用户看到的必须是可操作的原因，而不是「操作失败，请重试」。
        let failed: TaskResult<()> = Ok(Err(Error::Unauthorized));
        assert_eq!(task_failure(&failed, "测试"), Some("登录已失效，请重新登录"));

        let failed: TaskResult<()> = Ok(Err(Error::Network("timeout".into())));
        assert_eq!(task_failure(&failed, "测试"), Some("网络连接失败，请检查网络后重试"));
    }

    #[test]
    fn dropped_channel_falls_back_to_generic_message() {
        // 任务 panic / 运行时关闭：界面不能毫无反应。
        let failed: TaskResult<()> = Err(dropped_channel_error());
        assert_eq!(task_failure(&failed, "测试"), Some("操作未完成，请重试"));
    }
}
