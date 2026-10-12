//! 统一错误类型。
//!
//! 各模块的错误都应能转换到此类型，便于上层集中处理与呈现。

use std::fmt;

/// 统一错误。
#[derive(Debug)]
pub enum Error {
    /// 网络层失败（连接、TLS、超时等）。
    Network(String),
    /// 响应解码失败（JSON 反序列化等）。
    Decode(String),
    /// 网易云业务错误码。
    Api { code: i64, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(msg) => write!(f, "网络请求失败: {msg}"),
            Error::Decode(msg) => write!(f, "响应解析失败: {msg}"),
            Error::Api { code, message } => write!(f, "业务错误: code={code} message={message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Network(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Decode(e.to_string())
    }
}

/// 结果别名。
pub type Result<T> = std::result::Result<T, Error>;
