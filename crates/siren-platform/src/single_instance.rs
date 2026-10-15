//! 单实例：确保进程唯一；已有实例时唤起其窗口。
//!
//! 平台差异能力 → 根级统一函数契约 + 平台目录实现
//! （`windows/single_instance.rs` 命名互斥体；Linux / macOS 当前占位放行，待补）。

/// 唯一实例返回 `true`；已有实例则唤起其窗口并返回 `false`。
pub fn ensure_single_instance(app_id: &str) -> bool {
    crate::imp::ensure_single_instance(app_id)
}
