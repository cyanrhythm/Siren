//! macOS：单实例（当前占位放行）。
//!
//! TODO：以 `NSRunningApplication` 判定并激活已有实例（或 LaunchServices）；
//! 在此之前放行，不阻断启动。

/// 当前占位实现：始终返回 `true`（视为唯一实例）。
pub(crate) fn ensure_single_instance(_app_id: &str) -> bool {
    true
}
