//! 网络层：HTTP 客户端、请求签名、API 端点封装。
//!
//! 对应 Melodia 的 `core/network/`，参考 `docs/04-迁移规格.md § 5`。

pub mod client;
pub mod endpoints;
pub mod interceptor;

/// 网易云 API 返回的业务错误码处理。
pub mod api_error {
    // TODO: 定义业务错误码 → Error 的映射
}
