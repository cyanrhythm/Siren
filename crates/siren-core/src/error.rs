//! 统一错误类型。
//!
//! 语义对齐 Melodia 的 `core/network/AppError.kt`：上层按**类型**分流渲染，
//! 面向用户的文案由 [`Error::user_message`] 统一给出（对应 `AppErrorMessages.kt`）。

use std::fmt;

/// 统一错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 连接层失败（连接、TLS、超时等）——`AppError.NetworkError`。
    Network(String),
    /// 风控 / 0 字节响应等拦截器层失败——`AppError.RiskControl`。
    RiskControl,
    /// 未登录（业务码 301）——`AppError.Unauthorized`。
    Unauthorized,
    /// 响应解码失败（JSON 反序列化等）——`AppError.ParseError`。
    Parse(String),
    /// 其余未细分的业务失败兜底——`AppError.BizError`。
    Biz { code: i64, message: String },
}

impl Error {
    /// 由业务码构造：`301` 归一为 [`Error::Unauthorized`]，其余为 [`Error::Biz`]。
    pub fn from_code(code: i64, message: impl Into<String>) -> Self {
        if code == 301 {
            Error::Unauthorized
        } else {
            Error::Biz {
                code,
                message: message.into(),
            }
        }
    }

    /// 是否未登录。
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Unauthorized)
    }

    /// 面向用户的提示文案（对齐 `AppErrorMessages.kt`）。
    pub fn user_message(&self) -> &'static str {
        match self {
            Error::Network(_) => "网络连接失败，请检查网络后重试",
            Error::RiskControl => "请求被风控拦截，请稍后重试",
            Error::Unauthorized => "登录已失效，请重新登录",
            Error::Parse(_) => "数据解析失败",
            Error::Biz { .. } => "操作失败，请稍后重试",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(msg) => write!(f, "网络请求失败: {msg}"),
            Error::RiskControl => write!(f, "请求被风控拦截"),
            Error::Unauthorized => write!(f, "未登录（301）"),
            Error::Parse(msg) => write!(f, "响应解析失败: {msg}"),
            Error::Biz { code, message } => write!(f, "业务错误: code={code} message={message}"),
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
        Error::Parse(e.to_string())
    }
}

/// 结果别名。
pub type Result<T> = std::result::Result<T, Error>;

/// 「丢弃错误但留痕」：语义等同 `.ok()`，额外记一条 `warn` 日志。
///
/// 用于**失败可降级**的路径（可选数据、装饰信息、备选地址）：界面继续走空态，
/// 但日志里必须留下原因——否则线上只能看到「界面空着」，无从判断是接口失败、
/// 没数据，还是字段解析变了。用户可感知的失败**不应**用本方法，走 Toast。
pub trait ResultExt<T> {
    /// 转成 `Option<T>`；失败时以 `context` 为前缀记 `warn`。
    fn ok_or_warn(self, context: &str) -> Option<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn ok_or_warn(self, context: &str) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(err) => {
                log::warn!("{context}失败：{err}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_301_is_unauthorized() {
        assert!(Error::from_code(301, "need login").is_unauthorized());
    }

    #[test]
    fn other_codes_are_biz() {
        assert_eq!(
            Error::from_code(400, "bad"),
            Error::Biz {
                code: 400,
                message: "bad".to_string()
            }
        );
    }

    #[test]
    fn messages_are_distinct() {
        assert_ne!(
            Error::Unauthorized.user_message(),
            Error::Network(String::new()).user_message()
        );
    }

    #[test]
    fn ok_or_warn_keeps_value_and_drops_error() {
        let ok: Result<i32> = Ok(7);
        assert_eq!(ok.ok_or_warn("测试"), Some(7));

        let err: Result<i32> = Err(Error::Network("boom".into()));
        // 语义必须与 `.ok()` 完全一致，否则调用方的降级分支会被改变。
        assert_eq!(err.ok_or_warn("测试"), None);
    }
}
