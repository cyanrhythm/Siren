//! 音质档位。对应 Melodia `core/model/AudioQuality.kt`。
//!
//! 桌面端不做 WiFi/移动网络区分，仅保留单一档位设置。

use serde::{Deserialize, Serialize};

/// 音质档位（从低到高）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioQuality {
    Standard,
    Higher,
    #[default]
    ExHigh,
    Lossless,
    Hires,
    Jyeffect,
    Sky,
    Jymaster,
}

impl AudioQuality {
    /// 由低到高的全部档位。
    pub const ALL: [AudioQuality; 8] = [
        AudioQuality::Standard,
        AudioQuality::Higher,
        AudioQuality::ExHigh,
        AudioQuality::Lossless,
        AudioQuality::Hires,
        AudioQuality::Jyeffect,
        AudioQuality::Sky,
        AudioQuality::Jymaster,
    ];

    /// 接口用的档位代码。
    pub fn code(self) -> &'static str {
        match self {
            AudioQuality::Standard => "standard",
            AudioQuality::Higher => "higher",
            AudioQuality::ExHigh => "exhigh",
            AudioQuality::Lossless => "lossless",
            AudioQuality::Hires => "hires",
            AudioQuality::Jyeffect => "jyeffect",
            AudioQuality::Sky => "sky",
            AudioQuality::Jymaster => "jymaster",
        }
    }

    /// 展示文案（对齐 `getQualityDisplayName`）。
    pub fn display_name(self) -> &'static str {
        match self {
            AudioQuality::Standard => "标准",
            AudioQuality::Higher => "较高",
            AudioQuality::ExHigh => "极高",
            AudioQuality::Lossless => "无损 (FLAC)",
            AudioQuality::Hires => "Hi-Res",
            AudioQuality::Jyeffect => "高清环绕声",
            AudioQuality::Sky => "沉浸环绕声",
            AudioQuality::Jymaster => "超清母带",
        }
    }

    /// 该档位的编码类型。
    pub fn encode(self) -> &'static str {
        match self {
            AudioQuality::Lossless
            | AudioQuality::Hires
            | AudioQuality::Jyeffect
            | AudioQuality::Sky
            | AudioQuality::Jymaster => "flac",
            _ => "aac",
        }
    }

    /// 由代码解析；未知返回默认档。
    pub fn from_code(code: &str) -> AudioQuality {
        AudioQuality::ALL
            .into_iter()
            .find(|q| q.code() == code)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_roundtrip() {
        for q in AudioQuality::ALL {
            assert_eq!(AudioQuality::from_code(q.code()), q);
        }
    }

    #[test]
    fn unknown_falls_back_to_default() {
        assert_eq!(AudioQuality::from_code("nope"), AudioQuality::ExHigh);
    }

    #[test]
    fn lossless_uses_flac() {
        assert_eq!(AudioQuality::Lossless.encode(), "flac");
        assert_eq!(AudioQuality::ExHigh.encode(), "aac");
    }
}
