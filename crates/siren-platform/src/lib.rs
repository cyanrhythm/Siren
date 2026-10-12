//! # siren-platform
//!
//! 平台能力层：**唯一允许出现平台差异的 crate**。
//!
//! ## 组织方式
//!
//! - `src/*.rs`（根级）：平台能力 **trait 定义**，与平台无关
//! - `src/windows/`、`src/linux/`、`src/macos/`：各平台 **实现**
//!
//! ## 编译期保证
//!
//! 通过 `#[cfg(target_os = "...")]` 在编译期选择实现。
//! 若新增目标平台而未提供实现，会因符号未定义而**编译失败**——
//! 这是 Melodia 需要自建 KSP 处理器才能达到的效果，在 Rust 中是语言特性。
//!
//! ## 能力清单
//!
//! 详见 `docs/02-平台能力规格.md`。

// ── 平台能力 trait（与平台无关，始终编译）────────────────────────
pub mod accent;
pub mod app_paths;
pub mod lyric_window;
pub mod media_session;
pub mod ui_scale;
pub mod window;
// pub mod tray;          // TODO: 若 tray-icon 不够用再自建
// pub mod hotkey;        // TODO: 优先用 global-hotkey crate
// pub mod autostart;     // TODO: 优先用 auto-launch crate

// ── 平台实现（由 cfg 选择）───────────────────────────────────────
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

// ── 平台类型统一导出 ─────────────────────────────────────────────
//
// 上层通过这里的别名取用当前平台的实现，
// 无需关心具体平台。新增平台时在此补充分支。

// TODO: 待各 trait 与实现就绪后，补齐如下别名
//
// #[cfg(target_os = "windows")]
// pub use windows::media::WindowsMediaSession as PlatformMediaSession;
// #[cfg(target_os = "linux")]
// pub use linux::media::LinuxMediaSession as PlatformMediaSession;
// #[cfg(target_os = "macos")]
// pub use macos::media::MacMediaSession as PlatformMediaSession;

/// 编译期平台断言。
///
/// 若目标平台不在支持列表内，此模块会触发编译错误，
/// 避免出现"能编译但运行期无实现"的情况。
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
compile_error!(
    "Siren 目前仅支持 Windows / Linux / macOS。\
     若需支持新平台，请在 crates/siren-platform/src/ 下新增对应模块，\
     并在 lib.rs 中补充 cfg 分支与统一导出。"
);
