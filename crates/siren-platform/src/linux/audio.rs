//! Linux：音频输出设备前缀（按当前声音服务 PipeWire / PulseAudio / ALSA）。

/// 保留 PipeWire / PulseAudio / ALSA 输出设备。
pub(crate) fn device_prefixes() -> &'static [&'static str] {
    &["pipewire", "pulse", "alsa"]
}
