//! 单实例：确保进程唯一；已有实例时唤起其窗口。
//!
//! 平台差异能力 → 根级统一函数契约 + 平台目录实现：
//!
//! | 平台 | 机制 | 唤起窗口 |
//! |---|---|---|
//! | Windows | 命名互斥体（`CreateMutexW`） | 实现内直接完成（`FindWindowW` + `SetForegroundWindow`） |
//! | Linux | `$XDG_RUNTIME_DIR` 下 flock 锁文件 + 会话总线 | 经 D-Bus `Activate` 请求，**由主循环轮询后处理**（见下） |
//! | macOS | 占位放行（待补 `NSRunningApplication`） | — |
//!
//! ## 为什么 Linux 需要轮询
//!
//! 把已有实例的窗口带到前台，Wayland 下必须经 `xdg_activation_v1`（X11 下为
//! `_NET_ACTIVE_WINDOW`）——两者都要求**持有该窗口的图形连接**，句柄层无法代劳
//! （见 `docs/02-平台能力规格.md` §4 与平台层 `window_system.rs` 的降级说明）。
//! 因此 Linux 实现只把激活请求入队，由上层在主循环中经
//! [`poll_activation_events`] 取出，再用 GPUI 的 `Window::activate_window` 执行。
//! Windows 无此限制，实现内自行完成；其余平台不产生事件。

/// 单实例激活请求（「再次启动应用」时由已有实例收到）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationEvent {
    /// 请求把主窗口带到前台（如用户再次双击应用图标）。
    ActivateMainWindow,
}

/// 唯一实例返回 `true`；已有实例则请求唤起其窗口并返回 `false`。
///
/// 实现失败（如无会话总线、锁文件不可写）时按「唯一实例」放行，绝不阻断启动。
pub fn ensure_single_instance(app_id: &str) -> bool {
    crate::imp::ensure_single_instance(app_id)
}

/// 取走待处理的激活请求（非阻塞；在主循环中轮询，与托盘事件同模式）。
pub fn poll_activation_events(out: &mut Vec<ActivationEvent>) {
    crate::imp::poll_activation_events(out);
}
