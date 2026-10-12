//! EApi 加密：客户端接口使用。
//!
//! 流程：`nobody{url}use{text}md5forencrypt` 求 MD5，再拼成
//! `{url}-36cd479b6b5-{text}-36cd479b6b5-{digest}`，最后 AES-ECB，输出大写十六进制。

use md5::{Digest, Md5};

use crate::crypto::aes;

/// 加密 `text`（调用方需保证 `url` 为 `/api/` 前缀版本）。
pub fn eapi(url: &str, text: &str) -> String {
    let message = format!("nobody{url}use{text}md5forencrypt");
    let digest = md5_hex(&message);
    let data = format!("{url}-36cd479b6b5-{text}-36cd479b6b5-{digest}");
    aes::eapi_ecb_hex(data.as_bytes())
}

/// MD5，输出 32 位小写十六进制。
fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_uppercase_hex() {
        let a = eapi("/api/test", "{}");
        let b = eapi("/api/test", "{}");
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, a.to_uppercase());
    }

    #[test]
    fn differs_on_input() {
        assert_ne!(
            eapi("/api/song/enhance/player/url/v1", r#"{"ids":"[123]"}"#),
            eapi("/api/song/enhance/player/url/v1", r#"{"ids":"[456]"}"#)
        );
    }
}
