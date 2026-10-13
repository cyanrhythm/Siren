# 项目改进清单

> 来源：2025 年对 `crates/{siren-core,siren-platform,siren-app}` 的整体代码审查。
> 前提条件核查结果：clippy 零警告、73 个测试全部通过、工作区干净。

## 优先级排序

| # | 事项 | 优先级 | 状态 |
|---|---|---|---|
| 1 | 拆分 `Root` 上帝结构 | 🔴 高 | ⬜ |
| 2 | 落地 `state/mod.rs` 状态设计 | 🔴 高 | ⬜ |
| 3 | 清理未使用的 tokio `Runtime` 持有 | 🟡 中 | ⬜ |
| 4 | 清理过期注释与 TODO | 🟡 中 | ⬜ |
| 5 | 为 `player/mpv.rs` FFI 补测试 | 🟡 中 | ⬜ |
| 6 | 补播放队列边界测试 | 🟡 中 | ⬜ |
| 7 | 清理 workspace 中注释掉的待补依赖 | 🟢 低 | ⬜ |
| 8 | Linux 平台能力实机补齐 | 🟢 路线图 | ⬜ |

## 1. 拆分 `Root` 上帝结构 🔴

`siren-app/src/ui/mod.rs` 中的 `Root` 持有 30+ 字段，登录弹窗、评论面板、
歌单写操作、正在播放面板的状态全部混在一个 `Entity` 里。

**建议**：拆分为独立的子 Entity——

- `LoginOverlay`（login_open / login_tab / login_gen / qr_image / login_status / login_input）
- `CommentsPanel`（comments_* / comment_floor_*）
- `PlaylistActions`（create_open / picker_song / picker_playlists / rename_* / delete_pid / desc_*）
- `NowPlayingPanel`（now_playing_open / np_width / np_cards / np_album_page / np_simi_page）

每个子 Entity 自持状态并通过 `cx.emit` / `cx.subscribe` 与外壳通信，
`Root` 只保留路由（`nav`）、页面 Entity 和共享服务（`services`、`playback`）。

## 2. 落地 `state/mod.rs` 状态设计 🔴

`state/mod.rs` 目前只有设计注释与 `TODO: 设计状态容器与订阅机制`，
规划的 `PlaybackState` / `AuthState` / `LibraryState` 均未落地。
实际状态分散在 `Root`、`Playback` 与各页面中。与第 1 项配合推进。

## 3. 清理未使用的 tokio `Runtime` 持有 🟡

`Root._runtime: tokio::runtime::Runtime`（下划线即未用），仅实际使用了
`runtime.handle().clone()`。由根视图持有整条 Runtime 会延长其生命周期、
掩盖真实的依赖关系。改为只持有 `tokio::runtime::Handle`，Runtime 本身
留在 `main` / `run()` 中管理。

## 4. 清理过期注释与 TODO 🟡

- `siren-core/src/auth/mod.rs`：顶部注释仍写"扫码登录待后续补充"，
  但 `auth/qr.rs` 与 `login_dialog.rs` 已完整落地扫码流程（docs/08 已指出）。
- `siren-core/src/network/mod.rs:15`：`TODO: 定义业务错误码 → Error 的映射`
  已在 `network/api.rs` 实现，注释过时。

## 5. 为 `player/mpv.rs` FFI 补测试 🟡

libmpv 的 FFI 封装（`libloading` 动态加载 + 26 处 `unsafe`）目前零测试。
至少为可脱离句柄测试的纯逻辑部分补用例：

- 参数/选项序列化
- 返回码 → `Error` 的翻译
- 加载失败路径的降级行为（Silent 引擎接管）

## 6. 补播放队列边界测试 🟡

62 个测试全在 siren-core，但 `player/queue.rs` 的模式切换
（顺序 / 随机 / 单曲循环）缺边界用例：

- 随机模式下 `next`/`prev` 的行为与回环
- 模式切换时的队列与游标保持
- 队列尾部 / 头部推进
- 清空与重建队列

## 7. 清理 workspace 中注释掉的待补依赖 🟢

根 `Cargo.toml` 有大段注释掉的"待补充"依赖（tray-icon、global-hotkey、
auto-launch 等）。与当前路线图无关的可删除，避免误导；确属近期计划的
移到 `docs/07-开发计划.md` 对应条目下。

## 8. Linux 平台能力实机补齐 🟢（路线图）

- MPRIS（`linux/mpris/`）、SNI 托盘（`linux/sni/`）目前仍是骨架文件，
  KDE / GNOME 未实机验证。
- `siren-platform` 的占位模块（`accent.rs`、`app_paths.rs`、`window.rs`、
  `lyric_window.rs`）均为 0 行实现。
- 若目标是跨平台发布，这是当前最大的功能缺口（与 docs/06、docs/07 对齐）。
