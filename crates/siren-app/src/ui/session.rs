//! 账号会话（登录态）—— 跨页面共享的**唯一真源**。
//!
//! 登录成功后由 `LibraryPage` 写入；标题栏、歌单写操作等从此读取，
//! 不再各自保存昵称/头像/uid。约定见 `docs/10-状态管理约定.md`。

use gpui_kit::*;

/// 已登录账号。
#[derive(Clone)]
pub(crate) struct Account {
    pub uid: i64,
    pub nickname: SharedString,
    pub avatar_url: String,
}

/// 会话全局状态。
pub(crate) struct Session {
    account: Option<Account>,
}

impl Global for Session {}

impl Session {
    /// 启动时初始化（未登录）。
    pub(crate) fn init(cx: &mut App) {
        cx.set_global(Self { account: None });
    }

    /// 当前账号（未登录为 `None`）。
    pub(crate) fn account(cx: &App) -> Option<&Account> {
        cx.global::<Self>().account.as_ref()
    }

    /// 当前 uid（未登录为 `None`）。
    pub(crate) fn uid(cx: &App) -> Option<i64> {
        cx.global::<Self>().account.as_ref().map(|a| a.uid)
    }

    /// 设置 / 清除账号。
    pub(crate) fn set(cx: &mut App, account: Option<Account>) {
        cx.global_mut::<Self>().account = account;
    }
}
