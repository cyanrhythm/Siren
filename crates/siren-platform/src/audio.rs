//! 音频输出设备过滤：按当前平台的声音服务筛选 mpv 的 `audio-device-list`。
//!
//! mpv 的设备列表会混入其他后端的泛称条目（如 `openal`）。
//! 过滤前缀属平台差异 → 根级统一函数契约 + 平台目录实现
//! （Windows WASAPI / Linux PipeWire·PulseAudio·ALSA / macOS CoreAudio）；
//! **不能照搬 WASAPI 逻辑**（见 `docs/04-迁移规格.md § 3.5`）。

/// 当前平台应保留的音频设备名前缀。
pub fn device_prefixes() -> &'static [&'static str] {
    crate::imp::device_prefixes()
}
