//! 加密常量与随机密钥。
//!
//! 数值对照 Melodia 的 `NeteaseCrypto.kt`，其中 RSA 模数为**修正后的值**
//! （源码注释指出原值尾部存在拼写错误会导致解密失败）。

use rand::Rng;

/// AES-CBC 固定 IV。
pub const IV: &[u8; 16] = b"0102030405060708";

/// WEAPI 首次加密使用的预设密钥。
pub const PRESET_KEY: &[u8; 16] = b"0CoJUm6Qyw8W8jud";

/// EAPI 使用的主密钥。
pub const EAPI_KEY: &[u8; 16] = b"e82ckenh8dichen8";

/// LinuxAPI 使用的主密钥。
pub const LINUX_API_KEY: &[u8; 16] = b"rpaWUfe92PZ4WjM9";

/// RSA 公钥指数（十六进制）。
pub const PUBLIC_KEY: &str = "010001";

/// RSA 模数（十六进制，修正值）。
pub const MODULUS: &str = "e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";

const RANDOM_POOL: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// 生成 `len` 位随机密钥（`[a-zA-Z0-9]`）。
pub fn random_key(len: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| RANDOM_POOL[rng.gen_range(0..RANDOM_POOL.len())] as char)
        .collect()
}

/// 将 16 字节 ASCII 密钥转为定长数组。
pub fn fixed16(key: &str) -> &[u8; 16] {
    key.as_bytes().try_into().expect("AES 密钥必须为 16 字节 ASCII")
}
