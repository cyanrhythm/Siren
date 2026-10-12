//! 验证 libmpv 引擎：动态加载 + 播放音频。
//!
//! ```bash
//! # Windows
//! SIREN_LIBMPV=../Melodia/desktopApp/native/libmpv-2.dll \
//!   cargo run -p siren-core --example play -- <音频URL>
//! ```
//!
//! 未提供 `SIREN_LIBMPV` 时按各平台默认库名尝试。

use std::time::Duration;

use siren_core::player::mpv::Mpv;

fn main() {
    let candidates: Vec<String> = match std::env::var("SIREN_LIBMPV") {
        Ok(path) => vec![path],
        Err(_) => vec![
            "libmpv-2.dll".to_string(),
            "libmpv.so.2".to_string(),
            "libmpv.2.dylib".to_string(),
        ],
    };
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://www.soundhelix.com/examples/mp3/SoundHelix-Song-1.mp3".to_string());

    let mpv = match Mpv::load(&candidates) {
        Ok(mpv) => mpv,
        Err(e) => {
            eprintln!("加载 libmpv 失败：{e}");
            std::process::exit(1);
        }
    };

    mpv.set_volume(60.0);
    mpv.load_file(&url);
    println!("开始播放：{url}");

    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(500));
        match (mpv.position(), mpv.duration()) {
            (Some(pos), Some(dur)) => {
                println!("{pos:>6.1}s / {dur:>6.1}s");
                if dur > 0.0 && pos >= dur - 0.2 {
                    break;
                }
            }
            _ => println!("(等待元数据…)"),
        }
    }

    mpv.stop();
    println!("结束");
}
