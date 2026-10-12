//! MPRIS 2 实现（D-Bus）。
//!
//! ## 接口
//!
//! - `org.mpris.MediaPlayer2` —— 根接口（Identity / DesktopEntry / CanQuit 等）
//! - `org.mpris.MediaPlayer2.Player` —— 播放控制
//! - `org.freedesktop.DBus.Properties` —— 属性读写
//!
//! ## 总线名
//!
//! `org.mpris.MediaPlayer2.siren`
//!
//! ## 对象路径
//!
//! `/org/mpris/MediaPlayer2`
//!
//! ## ⚠️ 实现顺序很关键
//!
//! **必须先导出对象，再抢总线名。**
//! KDE 的 libkmpris 监听 `NameOwnerChanged`，名称一出现就立即发起 `GetAll`；
//! 若此时对象尚未导出，首次抓取失败会导致播放器被**永久忽略**。
//!
//! 详见 `docs/02-平台能力规格.md § 3.4`。
//!
//! ## 参考
//!
//! Melodia 的 `LinuxSystemMediaSession.kt`（371 行）可逐行对照移植。
//!
//! TODO: 实现
//! - 对象导出与总线名注册
//! - 元数据构造（注意 D-Bus 类型签名）
//! - 属性 Get / GetAll / Set
//! - PropertiesChanged 信号推送
//! - 指令方法（Play / Pause / Next / Previous / Seek / SetPosition）
