//! 加密模块：网易云 API 的请求签名算法。
//!
//! 对应 Melodia 的 `core/network/crypto/NeteaseCrypto.kt`。
//!
//! ## 三种加密方式
//!
//! | 方式 | 用途 |
//! |---|---|
//! | WeApi | Web / 小程序接口 |
//! | EApi | 客户端接口 |
//! | LinuxApi | Linux 客户端接口 |
//!
//! ## 关键参数
//!
//! - AES-CBC，固定 IV `0102030405060708`
//! - 预设密钥 `0CoJUm6Qyw8W8jud`（首次加密）
//! - 随机 16 位密钥（二次加密）
//! - RSA 公钥 `010001` + 特定模数
//!
//! ⚠️ **注意**：Melodia 源码中记录了 RSA 模数的**修正值**
//! （原值尾部存在拼写错误导致解密失败），移植时需确保使用正确值。
//!
//! ## 验收方式
//!
//! Melodia 有现成测试（`NeteaseCryptoTest` / `XeapiCryptoTest`），
//! 可作为 Rust 实现的对照验收标准。

pub mod eapi;
pub mod linuxapi;
pub mod weapi;

pub mod aes {
    // TODO: AES-CBC 加解密（固定 IV）
}

pub mod rsa {
    // TODO: RSA 加密（公钥 010001 + 模数）
}

pub mod key {
    // TODO: 随机密钥生成、预设密钥常量
}
