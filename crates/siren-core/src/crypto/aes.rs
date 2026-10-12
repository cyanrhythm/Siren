//! AES 加密封装：CBC（Base64 输出）与 ECB（大写十六进制输出），均为 PKCS#7 padding。

use aes::Aes128;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use cipher::{block_padding::Pkcs7, generic_array::GenericArray, BlockEncryptMut, KeyInit, KeyIvInit};

use crate::crypto::key::{EAPI_KEY, IV, LINUX_API_KEY, PRESET_KEY};

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128EcbEnc = ecb::Encryptor<Aes128>;

/// AES-128-CBC + PKCS#7，输出标准 Base64（无换行）。
pub fn cbc_base64(plaintext: &[u8], key: &[u8; 16]) -> String {
    let ciphertext = Aes128CbcEnc::new(GenericArray::from_slice(key), GenericArray::from_slice(IV))
        .encrypt_padded_vec_mut::<Pkcs7>(plaintext);
    STANDARD.encode(ciphertext)
}

/// AES-128-ECB + PKCS#7，输出大写十六进制。
pub fn ecb_hex_upper(plaintext: &[u8], key: &[u8; 16]) -> String {
    let ciphertext = Aes128EcbEnc::new(GenericArray::from_slice(key)).encrypt_padded_vec_mut::<Pkcs7>(plaintext);
    hex::encode_upper(ciphertext)
}

/// 用 WEAPI 预设密钥做一次 CBC（Base64）。
pub(crate) fn preset_cbc_base64(plaintext: &[u8]) -> String {
    cbc_base64(plaintext, PRESET_KEY)
}

/// EAPI 的 ECB（大写十六进制）。
pub(crate) fn eapi_ecb_hex(plaintext: &[u8]) -> String {
    ecb_hex_upper(plaintext, EAPI_KEY)
}

/// LinuxAPI 的 ECB（大写十六进制）。
pub(crate) fn linux_ecb_hex(plaintext: &[u8]) -> String {
    ecb_hex_upper(plaintext, LINUX_API_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 期望值由独立实现产生（openssl 3.6），用于确认与 Kotlin `NeteaseCrypto` 算法一致。
    #[test]
    fn cbc_matches_reference_vector() {
        // echo -n "hello" | openssl enc -aes-128-cbc -K 303132... -iv 30313032... -base64 -A
        assert_eq!(cbc_base64(b"hello", b"0123456789abcdef"), "FL1SM7AKsiwls1Ghnd/yEA==");
    }

    #[test]
    fn ecb_is_uppercase_hex() {
        let out = ecb_hex_upper(b"hello", EAPI_KEY);
        assert!(out.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(out, out.to_uppercase());
    }
}
