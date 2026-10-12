//! 请求拦截：在加密前向请求体注入公共字段、构造平台身份头/Cookie。
//!
//! 与 `client.rs` 分离，保持加密封装与 HTTP 传输的边界清晰，且均可在无网络下单测。

use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use serde_json::{json, Value};

use crate::network::device;

/// PC（WEAPI）User-Agent。
pub const PC_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// PC（WEAPI）Cookie 基础串。
const PC_COOKIE: &str = "os=pc; osver=Microsoft-Windows-10-Professional-build-10512-64bit; appver=3.0.1.201552";

/// 构造 WEAPI 明文载荷：注入 `csrf_token` 与 `e_r=false`。
pub fn weapi_payload(body: &Value, cookie: Option<&str>) -> String {
    let mut payload = body.clone();
    if let Value::Object(map) = &mut payload {
        map.insert("csrf_token".into(), json!(extract_csrf_token(cookie)));
        map.insert("e_r".into(), json!(false));
    }
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
}

/// 构造 EAPI 明文载荷：注入设备 `header` 与 `e_r=false`。
pub fn eapi_payload(body: &Value) -> String {
    let mut payload = body.clone();
    if let Value::Object(map) = &mut payload {
        map.insert("header".into(), build_eapi_header());
        map.insert("e_r".into(), json!(false));
    }
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
}

/// 从 Cookie 中提取 `__csrf`，无则空串。
pub fn extract_csrf_token(cookie: Option<&str>) -> String {
    cookie
        .and_then(|c| c.split(';').find_map(|p| p.trim().strip_prefix("__csrf=")))
        .unwrap_or("")
        .to_string()
}

/// WEAPI Cookie：存储 Cookie + PC 拟态。
pub fn weapi_cookie(stored: Option<&str>) -> String {
    match stored {
        Some(c) if !c.is_empty() => format!("{PC_COOKIE}; {c}"),
        _ => PC_COOKIE.to_string(),
    }
}

/// EAPI Cookie：剔除历史 `os=` 后追加 Android 身份。
pub fn eapi_cookie(stored: Option<&str>) -> String {
    let filtered = stored
        .unwrap_or("")
        .split("; ")
        .filter(|p| !p.trim().is_empty() && !p.trim().starts_with("os="))
        .collect::<Vec<_>>()
        .join("; ");
    let os = format!(
        "os=android; appver={}; osver={}",
        device::EAPI_APPVER,
        device::OS_RELEASE
    );
    if filtered.is_empty() {
        os
    } else {
        format!("{filtered}; {os}")
    }
}

fn build_eapi_header() -> Value {
    let millis = now_millis();
    let buildver: String = millis.to_string().chars().take(10).collect();
    json!({
        "osver": device::OS_RELEASE,
        "deviceId": device::device_id(),
        "os": "android",
        "appver": device::EAPI_APPVER,
        "versioncode": "140",
        "mobilename": device::MODEL,
        "buildver": buildver,
        "resolution": "1920x1080",
        "requestId": format!("{millis}_{}", rand::thread_rng().gen_range(1000..9999)),
    })
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapi_payload_injects_fields() {
        let text = weapi_payload(&json!({"s": "x"}), Some("__csrf=abc; MUSIC_U=1"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["csrf_token"], "abc");
        assert_eq!(value["e_r"], false);
    }

    #[test]
    fn eapi_payload_has_header() {
        let text = eapi_payload(&json!({"id": 1}));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["header"]["deviceId"], device::device_id());
        assert_eq!(value["header"]["os"], "android");
    }

    #[test]
    fn eapi_cookie_filters_os() {
        let cookie = eapi_cookie(Some("MUSIC_U=1; os=pc"));
        assert!(cookie.contains("MUSIC_U=1"));
        assert!(cookie.contains("os=android"));
        assert!(!cookie.contains("os=pc"));
    }
}
