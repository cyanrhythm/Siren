//! 系统媒体控制：同步播放信息到系统，并接收媒体键指令。
//!
//! ## 平台后端
//!
//! | 平台 | 后端 |
//! |---|---|
//! | Windows | SMTC（`SystemMediaTransportControls`） |
//! | Linux | MPRIS 2 over D-Bus |
//! | macOS | `MPNowPlayingInfoCenter` |
//!
//! ## 选型
//!
//! 优先尝试 `playwire` crate；若不满足则用 `zbus` 自研 MPRIS。
//! 详见 `docs/03-技术选型.md § 5`。
//!
//! ## 需求
//!
//! - 元数据：标题、歌手、专辑、封面、时长
//! - 状态：Playing / Paused / Stopped
//! - 进度：位置与总时长（带跳转检测）
//! - 模式：循环（单曲/列表）、随机
//! - 指令：播放、暂停、上下一首、定位、模式切换
//! - 可用性探测：后端不可用时优雅降级
//!
//! ## ⚠️ Linux MPRIS 实现要点（8 条）
//!
//! 以下来自 Melodia 在真实 KDE 环境踩过的坑，**换语言后依然适用**：
//!
//! 1. **先导出对象，再抢总线名** —— KDE 的 libkmpris 监听 `NameOwnerChanged`，
//!    名称一出现就立即 `GetAll`。若对象尚未导出，首次抓取失败会导致
//!    **播放器被永久忽略**（表现为能播放但媒体中心无卡片）
//! 2. 必须实现 `org.mpris.MediaPlayer2` **根接口**
//!    （`Identity` / `DesktopEntry` / `CanQuit` / `CanRaise` 等）
//! 3. 必须提供 `CanPause` / `CanStop` —— KDE 的暂停按钮会先读 `CanPause`，
//!    **缺失时直接短路**，根本不发出 D-Bus 调用
//! 4. `mpris:trackid` 必须是**对象路径类型**（签名 `o`），不是字符串
//! 5. `xesam:artist` 是**字符串数组**（签名 `as`）
//! 6. `mpris:length` 单位是**微秒**（签名 `x`，int64）
//! 7. 需主动推送 `PropertiesChanged` 信号，否则卡片刷新滞后
//! 8. `Set` 需接受 `LoopStatus` / `Shuffle` 写入
//!
//! ### D-Bus 类型签名速查
//!
//! | 字段 | 签名 |
//! |---|---|
//! | `mpris:trackid` | `o` |
//! | `mpris:length` | `x` |
//! | `mpris:artUrl` | `s` |
//! | `xesam:title` | `s` |
//! | `xesam:artist` | `as` |
//! | `xesam:album` | `s` |
//! | Metadata 整体 | `a{sv}` |

// TODO: 定义
// - trait MediaSession
// - struct MediaMetadata { title, artist, album, cover_url, duration_ms }
// - enum PlaybackStatus { Playing, Paused, Stopped }
// - enum MediaCommand { Play, Pause, Toggle, Next, Previous, SeekTo, SeekBy, SetRepeat, SetShuffle }
