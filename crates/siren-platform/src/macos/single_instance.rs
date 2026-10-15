//! macOS：单实例（当前占位放行）。
//!
//! TODO：以 `NSRunningApplication` 判定并激活已有实例（或 LaunchServices）；
//! 在此之前放行，不阻断启动。

/// 当前占位实现：始终返回 `true`（视为唯一实例）。
pub(crate) fn ensure_single_instance(_app_id: &str) -> bool {
    true
}

/// 占位期无激活事件（随单实例实现一并补）。
pub(crate) fn poll_activation_events(_out: &mut Vec<crate::ActivationEvent>) {}
