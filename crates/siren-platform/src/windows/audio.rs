//! Windows：音频输出设备前缀（WASAPI）。

/// 仅保留 WASAPI 输出设备（mpv 前缀 `wasapi/`）。
pub(crate) fn device_prefixes() -> &'static [&'static str] {
    &["wasapi"]
}
