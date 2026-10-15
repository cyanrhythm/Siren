//! 异步 HTTP 客户端（`reqwest` + rustls）。
//!
//! 支持两类请求：
//! - WEAPI：PC 身份，`music.163.com`
//! - EAPI：伪装 Android 身份，`interface.music.163.com`
//!
//! 请求体的加密在发送前完成，注入逻辑见 [`crate::network::interceptor`]。

use reqwest::Client;
use serde_json::Value;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::crypto;
use crate::error::Result;
use crate::network::{endpoints, interceptor};

/// 网易云客户端。`cookie` 为登录态（含 `MUSIC_U` 等），可在多份共享引用间更新。
#[derive(Debug, Clone)]
pub struct NeteaseClient {
    http: Client,
    cookie: Arc<RwLock<Option<String>>>,
    /// 真实 IP 伪装（对齐设置项 `useRealIp`）。
    pub real_ip: Option<String>,
}

impl Default for NeteaseClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NeteaseClient {
    pub fn new() -> Self {
        // 连接 10s、整体 30s 超时：接口挂起时请求会失败而非永久 pending（避免 UI 任务卡死）。
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http,
            cookie: Arc::new(RwLock::new(None)),
            real_ip: None,
        }
    }

    /// 构造带登录态的客户端。
    pub fn with_cookie(cookie: Option<String>) -> Self {
        Self {
            cookie: Arc::new(RwLock::new(cookie)),
            ..Self::new()
        }
    }

    /// 写入登录态（对共享的克隆可见）。
    pub fn set_cookie(&self, cookie: Option<String>) {
        if let Ok(mut guard) = self.cookie.write() {
            *guard = cookie;
        }
    }

    /// 读取登录态。
    pub fn cookie(&self) -> Option<String> {
        self.cookie.read().ok().and_then(|guard| guard.clone())
    }

    /// WEAPI POST：加密后以表单提交，返回解析后的 JSON。
    pub async fn weapi_post(&self, path: &str, body: &Value) -> Result<Value> {
        let cookie = self.cookie();
        let text = interceptor::weapi_payload(body, cookie.as_deref());
        let encrypted = crypto::weapi(&text);

        let url = format!("{}{path}", endpoints::WEB_BASE_URL);
        let mut request = self
            .http
            .post(url)
            .header("User-Agent", interceptor::PC_USER_AGENT)
            .header("Referer", endpoints::WEB_BASE_URL)
            .header("Cookie", interceptor::weapi_cookie(cookie.as_deref()));
        if let Some(ip) = &self.real_ip {
            request = request.header("X-Real-IP", ip).header("X-Forwarded-For", ip);
        }

        let response = request
            .form(&[
                ("params", encrypted.params.as_str()),
                ("encSecKey", encrypted.enc_sec_key.as_str()),
            ])
            .send()
            .await?;
        Self::parse_json(response).await
    }

    /// WEAPI POST，并额外返回响应头的 `Set-Cookie`（登录态请求用）。
    pub async fn weapi_post_with_cookie(&self, path: &str, body: &Value) -> Result<(Value, Option<String>)> {
        let cookie = self.cookie();
        let text = interceptor::weapi_payload(body, cookie.as_deref());
        let encrypted = crypto::weapi(&text);

        let url = format!("{}{path}", endpoints::WEB_BASE_URL);
        let mut request = self
            .http
            .post(url)
            .header("User-Agent", interceptor::PC_USER_AGENT)
            .header("Referer", endpoints::WEB_BASE_URL)
            .header("Cookie", interceptor::weapi_cookie(cookie.as_deref()));
        if let Some(ip) = &self.real_ip {
            request = request.header("X-Real-IP", ip).header("X-Forwarded-For", ip);
        }

        let response = request
            .form(&[
                ("params", encrypted.params.as_str()),
                ("encSecKey", encrypted.enc_sec_key.as_str()),
            ])
            .send()
            .await?;
        let set_cookie = collect_set_cookie(&response);
        let value = Self::parse_json(response).await?;
        Ok((value, set_cookie))
    }

    /// WEAPI 原始 multipart POST（不加密，用于头像上传等）。
    ///
    /// 对齐 Melodia `/weapi/user/avatar/upload/v1`：字段名 `imgFile`，仅带登录 Cookie。
    pub async fn weapi_multipart(
        &self,
        path: &str,
        field: &str,
        filename: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> Result<Value> {
        use reqwest::multipart::{Form, Part};

        let cookie = self.cookie();
        let part = Part::bytes(bytes).file_name(filename.to_string()).mime_str(mime)?;
        let form = Form::new().part(field.to_string(), part);

        let url = format!("{}{path}", endpoints::WEB_BASE_URL);
        let mut request = self
            .http
            .post(url)
            .header("User-Agent", interceptor::PC_USER_AGENT)
            .header("Referer", endpoints::WEB_BASE_URL)
            .header("Cookie", interceptor::weapi_cookie(cookie.as_deref()));
        if let Some(ip) = &self.real_ip {
            request = request.header("X-Real-IP", ip).header("X-Forwarded-For", ip);
        }

        let response = request.multipart(form).send().await?;
        Self::parse_json(response).await
    }

    /// EAPI POST：伪装 Android，仅提交 `params`。
    pub async fn eapi_post(&self, path: &str, body: &Value) -> Result<Value> {
        let api_path = path.replacen("/eapi/", "/api/", 1);
        let text = interceptor::eapi_payload(body);
        let params = crypto::eapi(&api_path, &text);

        let url = format!("{}{path}", endpoints::EAPI_BASE_URL);
        let response = self
            .http
            .post(url)
            .header("User-Agent", crate::network::device::eapi_user_agent())
            .header("Cookie", interceptor::eapi_cookie(self.cookie().as_deref()))
            .form(&[("params", params.as_str())])
            .send()
            .await?;
        Self::parse_json(response).await
    }

    /// 打卡上报专用 EAPI POST：Cookie 强制 `os=osx`（见 [`interceptor::eapi_cookie_weblog`]）。
    pub async fn eabi_post_weblog(&self, path: &str, body: &Value) -> Result<Value> {
        let api_path = path.replacen("/eapi/", "/api/", 1);
        let text = interceptor::eapi_payload(body);
        let params = crypto::eapi(&api_path, &text);

        let url = format!("{}{path}", endpoints::EAPI_BASE_URL);
        let response = self
            .http
            .post(url)
            .header("User-Agent", crate::network::device::eapi_user_agent())
            .header("Cookie", interceptor::eapi_cookie_weblog(self.cookie().as_deref()))
            .form(&[("params", params.as_str())])
            .send()
            .await?;
        Self::parse_json(response).await
    }

    async fn parse_json(response: reqwest::Response) -> Result<Value> {
        // 先校验状态码：502/风控 HTML、404 等不应被当成 JSON 解析。
        let response = response.error_for_status()?;
        let text = response.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// 下载二进制内容（封面等）。
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .header("User-Agent", interceptor::PC_USER_AGENT)
            .send()
            .await?;
        // 404/风控返回的 HTML 字节不应被当作封面图片。
        let response = response.error_for_status()?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// 按键合并两份 Cookie：`existing` 打底，`refresh` 中出现的键覆盖之。
///
/// 用于 `refresh_login`：服务端续期通常只重发 `MUSIC_U`，需保留旧 `__csrf` 等，
/// 否则整体替换后写操作接口可能被拒。
pub fn merge_cookies(existing: Option<&str>, refresh: &str) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut apply = |text: &str| {
        for pair in text.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let key = pair.split('=').next().unwrap_or(pair).trim().to_string();
            if !map.contains_key(&key) {
                order.push(key.clone());
            }
            map.insert(key, pair.to_string());
        }
    };
    if let Some(existing) = existing {
        apply(existing);
    }
    apply(refresh);
    order
        .iter()
        .filter_map(|k| map.get(k).cloned())
        .collect::<Vec<_>>()
        .join("; ")
}

/// 汇总响应头的 `Set-Cookie` 为一条 Cookie 串（仅取 `name=value`）。
fn collect_set_cookie(response: &reqwest::Response) -> Option<String> {
    let mut pairs = Vec::new();
    for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(text) = value.to_str() {
            if let Some(pair) = text.split(';').next().map(str::trim).filter(|s| !s.is_empty()) {
                pairs.push(pair.to_string());
            }
        }
    }
    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}
