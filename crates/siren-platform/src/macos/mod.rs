//! macOS 平台实现。
//!
//! | 能力 | 方案 |
//! |---|---|
//! | 目录 | `directories` crate |
//! | 媒体控制 | `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter` |
//! | 托盘 | `tray-icon` crate（`NSStatusItem`） |
//! | 快捷键 | `global-hotkey` crate（`CGEventTap`） |
//! | 自启 | `auto-launch` crate（Launch Agent） |
//! | 窗口装饰 | 系统原生装饰 |
//! | 强调色 | `NSColor.controlAccentColor` |
//! | 歌词穿透 | `NSWindow.ignoresMouseEvents` |
//!
//! ## 说明
//!
//! macOS 支持**可能排在首期之后**（见 `docs/06-路线图.md` 阶段 4）。
//! 但保留此目录，以确保平台抽象从一开始就是三平台的，
//! 而非事后补丁式地加入第三个平台。
//!
//! TODO: 补齐各模块
//!
//! pub mod media;      // MPNowPlayingInfoCenter
//! pub mod accent;     // NSColor
//! pub mod lyric;      // NSWindow

pub mod media;
pub mod native;
