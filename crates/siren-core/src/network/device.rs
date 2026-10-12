//! 设备标识与伪装信息。
//!
//! 对应 Melodia 的 `DeviceInfo.desktop.kt` / `NeteaseDeviceId.kt`：
//! 桌面端没有真实机型，统一伪装成同一台 Android 设备，保持身份一致。

use md5::{Digest, Md5};

pub const BRAND: &str = "HUAWEI";
pub const MODEL: &str = "HBN-AL00";
pub const BOARD: &str = "HBN";
pub const OS_RELEASE: &str = "12";
pub const EAPI_APPVER: &str = "9.0.90";

/// 设备标识：`md5("HUAWEI_HBN-AL00_HBN")`，小写十六进制。
pub fn device_id() -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("{BRAND}_{MODEL}_{BOARD}").as_bytes());
    hex::encode(hasher.finalize())
}

/// EAPI 请求使用的 Android User-Agent。
pub fn eapi_user_agent() -> String {
    format!("NeteaseMusic/9.0.90 (Linux; U; Android {OS_RELEASE}; zh_CN; {MODEL})")
}
