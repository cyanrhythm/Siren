//! GPUI 界面。
//!
//! ## 结构
//!
//! - `pages/`：页面级组件（首页、搜索、歌单、设置等）
//! - `components/`：可复用组件（播放条、曲目行、封面等）
//!
//! ## 待验证
//!
//! 以下 Melodia 的 UI 效果需在 PoC 阶段确认 gpui-kit 能否实现：
//!
//! | 效果 | gpui-kit 对应 |
//! |---|---|
//! | 长列表滚动 | Virtual Lists |
//! | 表格视图 | Data Tables |
//! | 侧栏布局 | Dock Layout |
//! | 歌词逐字动画 | 自绘 + 动画 API |
//! | 封面取色 | 需自实现算法 |
//!
//! 详见 `docs/05-PoC验证清单.md` P0-1。

pub mod components;
pub mod pages;

/// 主窗口根组件。
///
/// TODO: 实现
pub struct Root;
