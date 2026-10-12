//! 网易特化 RSA：明文**字节反转**后做大数模幂，密文左侧补零到 256 位十六进制。
//!
//! 该方案与标准 RSA 填充不同，故使用 `num-bigint` 直接做模幂，而非通用 `rsa` crate。

use num_bigint::BigUint;

use crate::crypto::key::{MODULUS, PUBLIC_KEY};

/// 加密 `text`，输出 256 位十六进制字符串。
pub fn rsa_encrypt(text: &str) -> String {
    let reversed: Vec<u8> = text.bytes().rev().collect();
    let m = BigUint::from_bytes_be(&reversed);
    let e = BigUint::parse_bytes(PUBLIC_KEY.as_bytes(), 16).expect("非法公钥指数");
    let n = BigUint::parse_bytes(MODULUS.as_bytes(), 16).expect("非法模数");
    let c = m.modpow(&e, &n);
    format!("{:0>256}", c.to_str_radix(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_256_hex() {
        let out = rsa_encrypt("0123456789abcdef");
        assert_eq!(out.len(), 256);
        assert!(out.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // 期望值由 Python 整数模幂独立计算得到。
    #[test]
    fn matches_reference_vector() {
        assert_eq!(
            rsa_encrypt("0123456789abcdef"),
            "35701388baf89fed412e11269b9c76625d095ecaf17f03fa018abe19ea2d38b949debf242ee39a71ca1f6cda71b1b86a45aa909ee27f7e78e267d34e732f0de948206c3340a788d0003372183e2f753c1f78b66ac23d134ac1fc9b993156520ea826b8aa89a962d4491b4b8d7e08738e1da9b07aa39bf4a7ef0b1c210728cd52"
        );
    }
}
