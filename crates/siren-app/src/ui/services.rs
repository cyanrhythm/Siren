//! 共享服务：跨页面复用的后端能力（网络客户端、运行时句柄、封面缓存）。
//!
//! 各页面持有其克隆，依赖共享服务而非外壳，符合 coding-guides 的依赖方向。

use std::sync::Arc;

use siren_core::network::client::NeteaseClient;

use super::covers::CoverLoader;

#[derive(Clone)]
pub(crate) struct Services {
    pub client: Arc<NeteaseClient>,
    pub handle: tokio::runtime::Handle,
    pub covers: CoverLoader,
}
