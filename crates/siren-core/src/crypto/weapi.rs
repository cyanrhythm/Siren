//! WeApi 加密：Web 端与小程序使用。
//!
//! 流程：
//! 1. 明文用**预设密钥** AES 加密（CBC，Base64）
//! 2. 第 1 步的 **Base64 字符串**再用**随机密钥** AES 加密，得到 `params`
//! 3. 随机密钥经 RSA 加密，得到 `encSecKey`
//!
//! ⚠️ 关键：第二次 AES 加密的输入是第一次加密后的 Base64 字符串，而非原始明文。

use serde::{Deserialize, Serialize};

use crate::crypto::{aes, key, rsa};

/// WEAPI 加密结果，对应表单里的 `params` 与 `encSecKey`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeapiParams {
    pub params: String,
    pub enc_sec_key: String,
}

/// 用随机 16 位密钥加密。
pub fn weapi(text: &str) -> WeapiParams {
    let secret = key::random_key(16);
    weapi_with_key(text, &secret)
}

/// 固定密钥版本，便于测试与复现。
pub fn weapi_with_key(text: &str, secret_key: &str) -> WeapiParams {
    let first = aes::preset_cbc_base64(text.as_bytes());
    let params = aes::cbc_base64(first.as_bytes(), key::fixed16(secret_key));
    WeapiParams {
        params,
        enc_sec_key: rsa::rsa_encrypt(secret_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_key_produces_two_fields() {
        let out = weapi(r#"{"foo":"bar"}"#);
        assert!(!out.params.is_empty());
        assert_eq!(out.enc_sec_key.len(), 256);
    }

    #[test]
    fn deterministic_for_same_input_and_key() {
        assert_eq!(
            weapi_with_key("hello", "0123456789abcdef"),
            weapi_with_key("hello", "0123456789abcdef")
        );
    }

    #[test]
    fn different_keys_differ() {
        let a = weapi_with_key("hello", "0123456789abcdef");
        let b = weapi_with_key("hello", "fedcba9876543210");
        assert_ne!(a.params, b.params);
        assert_ne!(a.enc_sec_key, b.enc_sec_key);
    }

    // 期望值经 openssl（两段 AES）+ Python（RSA 模幂）独立计算，确认与 Kotlin 实现一致。
    #[test]
    fn matches_reference_vector() {
        let out = weapi_with_key("hello", "0123456789abcdef");
        assert_eq!(out.params, "4fcL2vdE4wW3KbufvrJLNlTBoM3Fak+xPCAh3erUEPw=");
        assert_eq!(
            out.enc_sec_key,
            "35701388baf89fed412e11269b9c76625d095ecaf17f03fa018abe19ea2d38b949debf242ee39a71ca1f6cda71b1b86a45aa909ee27f7e78e267d34e732f0de948206c3340a788d0003372183e2f753c1f78b66ac23d134ac1fc9b993156520ea826b8aa89a962d4491b4b8d7e08738e1da9b07aa39bf4a7ef0b1c210728cd52"
        );
    }
}
