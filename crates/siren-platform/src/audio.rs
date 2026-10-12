//! 音频输出设备过滤：按当前平台的声音服务筛选 mpv 的 `audio-device-list`。
//!
//! mpv 的设备列表会混入其他后端的泛称条目（如 `openal`）。
//! 过滤规则按平台分支——**不能照搬 WASAPI 逻辑**（见 `docs/04-迁移规格.md § 3.5`）。

/// 当前平台应保留的音频设备名前缀。
#[cfg(target_os = "windows")]
pub const DEVICE_PREFIXES: &[&str] = &["wasapi"];

/// Linux：按当前声音服务（PipeWire / PulseAudio / ALSA）。
#[cfg(target_os = "linux")]
pub const DEVICE_PREFIXES: &[&str] = &["pipewire", "pulse", "alsa"];

/// macOS：CoreAudio。
#[cfg(target_os = "macos")]
pub const DEVICE_PREFIXES: &[&str] = &["coreaudio"];

/// 返回当前平台应保留的设备名前缀。
pub fn device_prefixes() -> &'static [&'static str] {
    DEVICE_PREFIXES
}
