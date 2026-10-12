//! Windows 平台实现。
//!
//! | 能力 | 方案 |
//! |---|---|
//! | 目录 | `directories` crate（跨平台，无需平台特定代码） |
//! | 媒体控制 | **SMTC**（`SystemMediaTransportControls`） |
//! | 托盘 | `tray-icon` crate |
//! | 快捷键 | `global-hotkey` crate |
//! | 自启 | `auto-launch` crate（注册表 `HKCU\...\Run`） |
//! | 窗口装饰 | DWM（`dwmapi`） |
//! | 强调色 | 注册表 `HKCU\Software\Microsoft\Windows\DWM` |
//! | 歌词穿透 | `WS_EX_TRANSPARENT \| WS_EX_LAYERED` |
//!
//! TODO: 补齐各模块
//!
//! pub mod media;      // SMTC
//! pub mod window;     // DWM
//! pub mod accent;     // 注册表
//! pub mod lyric;      // 窗口样式

pub mod native;
