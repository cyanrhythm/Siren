//! LinuxApi 加密：Linux 客户端接口使用。
//!
//! 直接用主密钥做 AES-ECB，输出大写十六进制。

use crate::crypto::aes;

/// 加密 `text`，输出大写十六进制。
pub fn linuxapi(text: &str) -> String {
    aes::linux_ecb_hex(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_uppercase_hex() {
        let a = linuxapi(r#"{"foo":"bar"}"#);
        let b = linuxapi(r#"{"foo":"bar"}"#);
        assert_eq!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, a.to_uppercase());
    }
}
