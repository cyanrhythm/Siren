//! 只读探测：专辑订阅状态（`resourceState`）与歌单简介字段是否如解析所假设。
//!
//! ```bash
//! cargo run -p siren-core --example album_state -- 1873541        # 专辑
//! cargo run -p siren-core --example album_state -- pl 24381616    # 歌单
//! ```

use siren_core::network::api;
use siren_core::network::client::NeteaseClient;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next().unwrap_or_default();
    let cookie = std::env::var("SIREN_COOKIE")
        .ok()
        .or_else(siren_core::auth::load_cookie);
    let client = NeteaseClient::with_cookie(cookie.clone());
    println!("登录态: {}", if cookie.is_some() { "有 Cookie" } else { "匿名" });

    if first == "pl" {
        let id: i64 = args.next().unwrap_or_default().parse().unwrap_or_default();
        match api::playlist_detail(&client, id).await {
            Ok(Some(d)) => println!(
                "歌单 {id}：subscribed={} description={:?} 名称={:?}",
                d.subscribed, d.description, d.name
            ),
            Ok(None) => println!("歌单 {id}：无数据"),
            Err(e) => println!("歌单 {id} 错误: {e}"),
        }
        return;
    }

    if first == "list" {
        match api::album_sublist(&client).await {
            Ok(list) => {
                println!("收藏的专辑 {} 张：", list.len());
                for a in list.iter().take(8) {
                    // 逐张查详情，看 resourceState 与列表是否一致
                    let state = api::album_detail(&client, a.id)
                        .await
                        .map(|d| d.map(|d| d.subscribed))
                        .unwrap_or(None);
                    println!(
                        "  [{}] {} — 列表名={:?} 详情 subscribed={:?}",
                        a.id, a.name, a.name, state
                    );
                }
            }
            Err(e) => println!("album_sublist 错误: {e}"),
        }
        return;
    }

    let id: i64 = first.parse().unwrap_or_default();
    match api::album_detail(&client, id).await {
        Ok(Some(d)) => println!("专辑 {id}：subscribed={} 名称={:?}", d.subscribed, d.name),
        Ok(None) => println!("专辑 {id}：无数据"),
        Err(e) => println!("专辑 {id} 错误: {e}"),
    }
}
