//! 窗口装饰：无边框窗口的系统外观。
//!
//! ## 平台方案
//!
//! | 平台 | 方案 |
//! |---|---|
//! | Windows | `appears_transparent` 隐藏系统标题栏 + 系统 NC 命中测试；不请求 CSD |
//! | Linux | **请求 CSD**（[`prefers_client_decorations`]）：合成器不再画标题栏（否则 Wayland 下双标题栏）；阴影 / 缩放边缘 / client inset 由 gpui-component 的 `window_border` 承担；X11 无合成器时 GPUI 自动回退服务端装饰 |
//! | macOS | 系统原生装饰（待补） |
//!
//! ## 原则
//!
//! 装饰类能力**不应阻塞核心功能**，缺失时降级为默认外观即可。
//! 上层据 `Window::window_decorations()`（GPUI 运行期上报的实际模式，
//! 可能因合成器拒绝 CSD 而回退为服务端装饰）决定是否启用自绘拖动/阴影。
//!
//! TODO（装饰效果增强，未建模）：`enable_system_animations` / `apply_frame(maximized, border_rgb)`。

/// 是否应由应用侧绘制窗口装饰（CSD，客户端装饰）。
///
/// - Linux 恒 `true`：自绘标题栏 + 自绘阴影/缩放边缘（`window_border`）；
///   X11 无合成器时 GPUI 会自行回退为服务端装饰。
/// - Windows / macOS 为 `false`（前者走系统标题栏隐藏 + NC 命中测试，后者待补）。
pub fn prefers_client_decorations() -> bool {
    crate::imp::prefers_client_decorations()
}
