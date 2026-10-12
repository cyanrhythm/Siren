//! playwire PoC：在 Windows 上验证 SMTC 可用（`docs/05-PoC验证清单.md` P0-3）。
//!
//! ```bash
//! cargo run -p siren-platform --example media_poc
//! ```
//!
//! 运行后应在 Windows 媒体浮层出现 “Siren / PoC”。非 Windows 平台请用 MPRIS 验证。

#[cfg(target_os = "windows")]
fn main() {
    unsafe { run_windows() };
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("media_poc：非 Windows 平台，请改在 Linux(KDE/GNOME) 验证 MPRIS");
}

#[cfg(target_os = "windows")]
unsafe fn run_windows() {
    use std::time::{Duration, Instant};

    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, PeekMessageW, TranslateMessage, CW_USEDEFAULT, MSG, PM_REMOVE,
        WS_OVERLAPPEDWINDOW,
    };

    // 用系统内置 STATIC 类创建顶层窗口，省去自注册（避免额外 feature）。
    let hwnd = CreateWindowExW(
        Default::default(),
        w!("STATIC"),
        w!("SirenMediaPoc"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        400,
        300,
        None,
        None,
        None,
        None,
    )
    .expect("CreateWindowExW");
    println!("HWND = {:p}", hwnd.0);

    let config = playwire::PlayerConfig::new("Siren")
        .desktop_entry("com.cyanrhythm.Siren")
        .track_id_prefix("/com/cyanrhythm/Siren/track")
        .hwnd(hwnd.0 as u64);

    match playwire::MediaControls::new(config, |event| println!("[event] {event:?}")) {
        Ok(mut controls) => {
            let state = playwire::PlaybackState {
                track: Some(playwire::Track {
                    id: "poc".into(),
                    title: "PoC".into(),
                    artists: vec!["Siren".into()],
                    album: "test".into(),
                    artwork_url: String::new(),
                    url: String::new(),
                }),
                playing: true,
                position: Duration::from_secs(0),
                duration: Some(Duration::from_secs(90)),
                volume: 1.0,
                repeat: playwire::Repeat::Off,
                shuffle: false,
                capabilities: playwire::Capabilities {
                    can_go_next: true,
                    can_go_previous: true,
                    can_seek: true,
                },
            };
            match controls.set_state(&state) {
                Ok(()) => println!("set_state OK —— 应可在 Windows 媒体浮层看到 Siren / PoC"),
                Err(e) => println!("set_state 失败: {e}"),
            }

            let start = Instant::now();
            let mut msg = MSG::default();
            while start.elapsed() < Duration::from_secs(6) {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            println!("PoC 结束");
        }
        Err(e) => println!("MediaControls::new 失败: {e}"),
    }
}
