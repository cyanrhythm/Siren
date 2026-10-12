//! 验证异步网络层：搜索 + 音源 + 账号。
//!
//! ```bash
//! cargo run -p siren-core --example search -- 周杰伦
//! # 带登录态对比音质
//! SIREN_COOKIE='MUSIC_U=xxx' cargo run -p siren-core --example search -- 周杰伦
//! ```

use siren_core::network::api;
use siren_core::network::client::NeteaseClient;

#[tokio::main]
async fn main() {
    let keyword = std::env::args().nth(1).unwrap_or_else(|| "周杰伦".to_string());
    let cookie = std::env::var("SIREN_COOKIE")
        .ok()
        .or_else(siren_core::auth::load_cookie);
    let client = NeteaseClient::with_cookie(cookie.clone());
    println!("登录态: {}", if cookie.is_some() { "有 Cookie" } else { "匿名" });

    match api::account(&client).await {
        Ok(v) => {
            let nick = v
                .get("profile")
                .and_then(|p| p.get("nickname"))
                .and_then(|n| n.as_str())
                .unwrap_or("(未登录)");
            println!("account code={:?} nickname={nick}", v.get("code"));
        }
        Err(e) => println!("account 错误: {e}"),
    }

    match api::search(&client, &keyword, 1, 5).await {
        Ok(songs) => {
            println!("搜索「{keyword}」-> {} 首", songs.len());
            for s in &songs {
                println!("  [{}] {} — {}", s.id, s.name, s.artists);
            }
            if let Some(first) = songs.first() {
                match api::song_url_eapi(&client, first.id, "exhigh").await {
                    Ok(Some(url)) => {
                        println!("  音源: {}", url.chars().take(80).collect::<String>())
                    }
                    Ok(None) => println!("  音源: 无（无版权/需登录）"),
                    Err(e) => println!("  音源错误: {e}"),
                }
            }
        }
        Err(e) => println!("搜索错误: {e}"),
    }
}
