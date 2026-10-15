//! macOS：音频输出设备前缀（CoreAudio）。

/// 保留 CoreAudio 输出设备。
pub(crate) fn device_prefixes() -> &'static [&'static str] {
    &["coreaudio"]
}
