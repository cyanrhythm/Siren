//! 系统媒体控制：同步播放信息到系统，并接收媒体键指令。
//!
//! 后端统一使用 [`playwire`]（MPRIS / SMTC / Now Playing 一套 API）；
//! 平台差异只体现在[窗口句柄解析](crate::PlatformMediaSession)上。
//!
//! ## ⚠️ Linux MPRIS 实现要点
//!
//! playwire 用 zbus 实现 MPRIS，已处理 Melodia 踩过的坑（对象先导出再抢总线名、
//! 根接口、`CanPause`/`CanStop`、trackid 类型、artist 数组、length 微秒、
//! `PropertiesChanged`、`Set` 接受 LoopStatus/Shuffle）。仍需在 KDE/GNOME 实测。

use std::sync::Mutex;
use std::time::Duration;

use siren_core::player::PlayMode;

/// 当前播放元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaMetadata {
    /// 唯一 id（MPRIS trackid 用）。
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub artwork_url: Option<String>,
    pub url: Option<String>,
    pub duration_ms: Option<u64>,
}

/// 播放状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    #[default]
    Paused,
    Stopped,
}

/// 系统媒体指令。
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    /// 绝对跳转（毫秒）。
    SeekTo(u64),
    /// 相对跳转（毫秒，可负）。
    SeekBy(i64),
    /// 音量（0.0..=1.0）。
    SetVolume(f64),
    SetShuffle(bool),
    /// true = 单曲循环。
    SetRepeat(bool),
}

/// 系统媒体控制会话。
pub trait MediaSession: Send + Sync {
    /// 后端是否可用。
    fn is_available(&self) -> bool;
    /// 更新元数据。
    fn set_metadata(&self, metadata: &MediaMetadata);
    /// 更新播放状态。
    fn set_playback_status(&self, status: PlaybackStatus);
    /// 更新进度（毫秒）。
    fn set_timeline(&self, position_ms: u64, duration_ms: u64);
    /// 更新播放模式。
    fn set_mode(&self, mode: PlayMode);
    /// 释放资源。
    fn shutdown(&self);
}

/// 会话配置。
#[derive(Debug, Clone, Default)]
pub struct MediaSessionConfig {
    pub identity: String,
    pub desktop_entry: Option<String>,
    pub app_media_id: Option<String>,
    pub track_id_prefix: Option<String>,
    /// 已解析的原生窗口句柄（Windows SMTC 需要；其他平台忽略）。
    pub hwnd: Option<u64>,
}

#[derive(Default)]
struct Snapshot {
    metadata: MediaMetadata,
    status: PlaybackStatus,
    position_ms: u64,
    duration_ms: u64,
    repeat: playwire::Repeat,
    shuffle: bool,
}

/// playwire 后端（跨平台）。
pub struct PlaywireMediaSession {
    controls: Mutex<playwire::MediaControls>,
    snapshot: Mutex<Snapshot>,
}

impl PlaywireMediaSession {
    /// 创建会话。`on_command` 从 OS 线程回调，务必快速返回（勿阻塞）。
    pub fn new(
        config: MediaSessionConfig,
        on_command: Box<dyn Fn(MediaCommand) + Send + Sync>,
    ) -> Result<Self, playwire::Error> {
        let mut pcfg = playwire::PlayerConfig::new(config.identity);
        if let Some(entry) = config.desktop_entry {
            pcfg = pcfg.desktop_entry(entry);
        }
        if let Some(prefix) = config.track_id_prefix {
            pcfg = pcfg.track_id_prefix(prefix);
        }
        if let Some(id) = config.app_media_id {
            pcfg = pcfg.app_media_id(id);
        }
        if let Some(hwnd) = config.hwnd {
            pcfg = pcfg.hwnd(hwnd);
        }

        let controls = playwire::MediaControls::new(pcfg, move |event| {
            if let Some(command) = map_event(&event) {
                on_command(command);
            }
        })?;

        Ok(Self {
            controls: Mutex::new(controls),
            snapshot: Mutex::new(Snapshot::default()),
        })
    }

    fn publish(&self) {
        let state = {
            let snap = self.snapshot.lock().unwrap();
            let track = (!snap.metadata.title.is_empty()).then(|| playwire::Track {
                id: snap.metadata.id.clone(),
                title: snap.metadata.title.clone(),
                artists: snap.metadata.artists.clone(),
                album: snap.metadata.album.clone(),
                artwork_url: snap.metadata.artwork_url.clone().unwrap_or_default(),
                url: snap.metadata.url.clone().unwrap_or_default(),
            });
            playwire::PlaybackState {
                track,
                playing: matches!(snap.status, PlaybackStatus::Playing),
                position: Duration::from_millis(snap.position_ms),
                duration: (snap.duration_ms > 0).then(|| Duration::from_millis(snap.duration_ms)),
                volume: 1.0,
                repeat: snap.repeat,
                shuffle: snap.shuffle,
                capabilities: playwire::Capabilities {
                    can_go_next: true,
                    can_go_previous: true,
                    can_seek: true,
                },
            }
        };
        if let Ok(mut controls) = self.controls.lock() {
            let _ = controls.set_state(&state);
        }
    }
}

impl MediaSession for PlaywireMediaSession {
    fn is_available(&self) -> bool {
        true
    }

    fn set_metadata(&self, metadata: &MediaMetadata) {
        self.snapshot.lock().unwrap().metadata = metadata.clone();
        self.publish();
    }

    fn set_playback_status(&self, status: PlaybackStatus) {
        self.snapshot.lock().unwrap().status = status;
        self.publish();
    }

    fn set_timeline(&self, position_ms: u64, duration_ms: u64) {
        {
            let mut snap = self.snapshot.lock().unwrap();
            snap.position_ms = position_ms;
            snap.duration_ms = duration_ms;
        }
        self.publish();
    }

    fn set_mode(&self, mode: PlayMode) {
        {
            let mut snap = self.snapshot.lock().unwrap();
            match mode {
                PlayMode::ListLoop => {
                    snap.repeat = playwire::Repeat::All;
                    snap.shuffle = false;
                }
                PlayMode::SingleLoop => {
                    snap.repeat = playwire::Repeat::One;
                    snap.shuffle = false;
                }
                PlayMode::Shuffle => {
                    snap.repeat = playwire::Repeat::Off;
                    snap.shuffle = true;
                }
            }
        }
        self.publish();
    }

    fn shutdown(&self) {}
}

fn map_event(event: &playwire::Event) -> Option<MediaCommand> {
    use playwire::Event;
    Some(match event {
        Event::Play => MediaCommand::Play,
        Event::Pause => MediaCommand::Pause,
        Event::PlayPause => MediaCommand::Toggle,
        Event::Stop => MediaCommand::Stop,
        Event::Next => MediaCommand::Next,
        Event::Previous => MediaCommand::Previous,
        Event::SeekTo(d) => MediaCommand::SeekTo(d.as_millis() as u64),
        Event::SeekBy(seconds) => MediaCommand::SeekBy((*seconds * 1000.0) as i64),
        Event::SetVolume(v) => MediaCommand::SetVolume(*v),
        Event::SetShuffle(shuffle) => MediaCommand::SetShuffle(*shuffle),
        Event::SetRepeat(repeat) => MediaCommand::SetRepeat(matches!(repeat, playwire::Repeat::One)),
        // OpenUri / Raise / Quit 及未来变体：暂不处理
        _ => return None,
    })
}
