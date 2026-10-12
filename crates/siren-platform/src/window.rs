//! 窗口装饰：无边框窗口的系统外观。
//!
//! ## 平台方案
//!
//! | 平台 | 方案 |
//! |---|---|
//! | Windows | DWM 圆角、边框色、最小化动画 |
//! | Linux | 交给窗口管理器 / CSD（**不建议自行干预**） |
//! | macOS | 系统原生装饰 |
//!
//! ## 原则
//!
//! 装饰能力**不应阻塞核心功能**，缺失时降级为默认外观即可。
//!
//! TODO: 定义 trait
//! - enable_system_animations()
//! - apply_frame(maximized, border_rgb)
