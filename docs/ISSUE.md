# ISSUE 整改清单

> 本清单记录已确认的问题与整改项，边整改边勾选。
> 约定文档见 [`10-状态管理约定.md`](10-状态管理约定.md)。

图例：`[ ]` 待办 · `[x]` 已完成 · **P0** 阻断/高价值 · P1 应做 · P2 可延后

---

## A. 状态管理（P0）

- [x] **A1 Root 上帝对象**：`ui/mod.rs:163` 57 个字段，混合导航/登录/评论/歌单写操作/正在播放配置/账号。
  拆分为 `NavState` / `LoginDialogState` / `CommentsPanelState` / `PlaylistOpsState` / `NowPlayingUi`。
- [x] **A2 设置无单一来源**：持久化是散装 `settings::{load,save}_*`（`siren-core/src/settings.rs`），
  同一 `settings.json` 被 `Playback`（quality/media_keys）、`Root`（np_width/np_cards）、`lib.rs`（window）三处拥有。
  → 统一为一个 `Settings` 模型 + 应用层全局 `SettingsStore`（GPUI Global）。
- [x] **A3 账号双真源**：`Root.account`（`ui/mod.rs:189`）与 `LibraryPage.{logged_in,nickname,avatar_url,uid}`（`pages/library.rs:22`），
  靠 `PageEvent::Account/LoggedOut` 手动同步。→ 收敛为单一 `Session`（全局）。
- [x] **A4 评论**：复核后二者语义不同（`Playback.comments` 是当前歌预览卡；`Root.comments` 是指定目标的浮层面板），保留。
- [x] **A5 订阅/喜欢态**：复核后语义不同（歌曲喜欢 / 歌单订阅 / 专辑订阅），分属各 owner，保留。
- [x] **A6 路由状态**：导航已收敛为 `NavState`；详情页句柄随路由 push/back 统一清建。
- [x] **A7 路由**：确认设计为「顶层页切换重置历史、详情页 push」；已封装为 `NavState`（含 `route/reset/push/pop`）。

## B. 异步与健壮性（P1）

- [x] **B1 页面异步无取消**：为 Playlist/Album/Recent/Search/Library 的加载加 generation guard（登录二维码已有 `gen`）。
- [~] **B2 `detach()`**：已为页面加载加代判；其余发射后不管流程待逐步补失败日志。

## C. 文档（P0）

- [x] **C1** 新增 [`10-状态管理约定.md`](10-状态管理约定.md)：owner 层级、单一真源、持久化归属、事件/订阅、异步取消规范。
- [x] **C2** 日志库与轮转：`siren_core::logging`（`log` 门面，1 MiB 轮转、保留 3 份）。
- [ ] **C3** `docs/01 §4.1` 仅覆盖播放状态单向流，补链到 `docs/10`。

## E. 错误提示 / 通知（P0）

- [x] **E1 全局 Toast**：新增 `siren-app::ui::Notifications`（Entity，唯一真源）+ `Root` 底部居中浮层
  （播放条上方，自动消失、去重、最多 3 条）。`Services` 暴露 `toast/toast_success/toast_error`。
  已覆盖：登录过期/失败、无可用播放地址、喜欢失败、歌单新建/重命名/删除/简介/添加到歌单、歌单与专辑订阅。
- [ ] **E2 收敛残留**：把各页 `status` 文字与静默 `.ok()`（约 198 处）逐步改为 Toast / 日志，
  接入 `siren_core::error::Error::user_message()` 文案。

## D. 其他（P2）

- [x] **D1 `Playback` UI 偏好**：`quality`/`media_keys` 已迁至设置 store（A2）。
- [~] **D2 `pages/settings.rs` 仍为 `impl Root`**：随 A1/A2 已改为只读全局 store，暂不再拆独立实体。
- [ ] **D3 平台未验证**：Linux/macOS 的 playwire/MPRIS 路径尚无实机验证。
