//! 系统强调色。
//!
//! ## 平台来源
//!
//! | 平台 | 来源 |
//! |---|---|
//! | Windows | 注册表 `HKCU\Software\Microsoft\Windows\DWM` 的 `AccentColor` |
//! | Linux | GTK：`~/.config/gtk-3.0/settings.ini`<br/>KDE：`~/.config/kdeglobals` |
//! | macOS | `NSColor.controlAccentColor` |
//!
//! 取不到时返回 `None`，由上层回退到默认主题色。
//!
//! 注意：Linux 上"读配置文件"**不等于**引入 GTK 依赖——只是解析 INI 文件。
//!
//! TODO: 定义 trait
//! - window_border_accent() -> Option<u32>   // 0xRRGGBB
