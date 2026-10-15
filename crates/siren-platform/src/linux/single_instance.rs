//! Linux：单实例（当前占位放行）。
//!
//! TODO：以 `$XDG_RUNTIME_DIR` 下的 flock 锁判定唯一实例；已有实例时
//! 经 D-Bus 激活其窗口（与 SNI/MPRIS 同栈）。在此之前放行，不阻断启动。

/// 当前占位实现：始终返回 `true`（视为唯一实例）。
pub(crate) fn ensure_single_instance(_app_id: &str) -> bool {
    true
}
