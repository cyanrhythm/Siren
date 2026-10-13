# ISSUE 整改清单

> 本清单记录已确认的问题与整改项，边整改边勾选。
> 约定文档见 [`10-状态管理约定.md`](10-状态管理约定.md)。

图例：`[ ]` 待办 · `[x]` 已完成 · **P0** 阻断/高价值 · P1 应做 · P2 可延后

---

## A. 状态管理（P0）

- [ ] **A1 Root 上帝对象**：`ui/mod.rs:163` 57 个字段，混合导航/登录/评论/歌单写操作/正在播放配置/账号。
  拆分为 `NavState` / `LoginDialogState` / `CommentsPanelState` / `PlaylistOpsState` / `NowPlayingUi`。
- [ ] **A2 设置无单一来源**：持久化是散装 `settings::{load,save}_*`（`siren-core/src/settings.rs`），
  同一 `settings.json` 被 `Playback`（quality/media_keys）、`Root`（np_width/np_cards）、`lib.rs`（window）三处拥有。
  → 统一为一个 `Settings` 模型 + 应用层全局 `SettingsStore`（GPUI Global）。
- [ ] **A3 账号双真源**：`Root.account`（`ui/mod.rs:189`）与 `LibraryPage.{logged_in,nickname,avatar_url,uid}`（`pages/library.rs:22`），
  靠 `PageEvent::Account/LoggedOut` 手动同步。→ 收敛为单一 `Session`（全局）。
- [ ] **A4 评论双真源**：`Playback.comments`（预览）与 `Root.comments_*`（面板）两套。
- [ ] **A5 订阅态分散**：`Playback.liked`、`PlaylistPage.subscribed`、`AlbumPage.subscribed` 各存一份。
- [ ] **A6 路由状态并行**：`nav: Vec<Route>` 与 `playlist_page/album_page: Option<Entity>` 并行维护。
- [ ] **A7 `navigate` 清空历史**：`ui/mod.rs:392` 每次 `nav.clear()`，`back` 形同虚设（应在首次进入推送）。

## B. 异步与健壮性（P1）

- [ ] **B1 页面异步无取消**：`reload*` 无 generation guard，快速切换/重载可被旧响应覆盖（仅二维码登录用了 `login_gen`）。
- [ ] **B2 `detach()` 泛滥**：33 处 `.detach()`，错误与取消被忽略；至少记录失败日志。

## C. 文档（P0）

- [x] **C1** 新增 [`10-状态管理约定.md`](10-状态管理约定.md)：owner 层级、单一真源、持久化归属、事件/订阅、异步取消规范。
- [x] **C2** 日志库与轮转：`siren_core::logging`（`log` 门面，1 MiB 轮转、保留 3 份）。
- [ ] **C3** `docs/01 §4.1` 仅覆盖播放状态单向流，补链到 `docs/10`。

## D. 其他（P2）

- [ ] **D1 `Playback` 承担 UI 偏好**：`media_keys_enabled` 属设置而非播放服务（随 A2 迁移）。
- [ ] **D2 `pages/settings.rs` 实为 `impl Root`**：设置页不是独立实体，逻辑回灌外壳（随 A1/A2 收敛）。
- [ ] **D3 平台未验证**：Linux/macOS 的 playwire/MPRIS 路径尚无实机验证。
