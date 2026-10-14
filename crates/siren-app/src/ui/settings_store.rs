//! 应用级设置 store —— 跨页面共享的**唯一真源**。
//!
//! 以 GPUI `Global` 承载 [`siren_core::settings::Settings`]，各处只读
//! `cx.global::<SettingsStore>()`、经其方法变更（内部负责落盘）。
//! 约定见 `docs/10-状态管理约定.md`。

use gpui_kit::*;

use siren_core::model::AudioQuality;
use siren_core::settings::Settings;

/// 设置唯一实例。
pub(crate) struct SettingsStore {
    data: Settings,
}

impl Global for SettingsStore {}

impl SettingsStore {
    /// 启动时装载一次。
    pub(crate) fn init(cx: &mut App) {
        cx.set_global(Self { data: Settings::load() });
    }

    /// 读取当前全局实例。
    pub(crate) fn of(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// 音质档位。
    pub(crate) fn quality(&self) -> AudioQuality {
        self.data.quality()
    }

    /// 设置音质档位（落盘）。
    pub(crate) fn set_quality(&mut self, quality: AudioQuality) {
        self.data.set_quality(quality);
    }

    /// 系统媒体控制开关。
    pub(crate) fn media_keys(&self) -> bool {
        self.data.media_keys()
    }

    /// 设置系统媒体控制开关（落盘）。
    pub(crate) fn set_media_keys(&mut self, enabled: bool) {
        self.data.set_media_keys(enabled);
    }

    /// 正在播放卡片完整顺序（含隐藏项）。
    pub(crate) fn card_order(&self) -> Vec<String> {
        self.data.card_order()
    }

    /// 某卡片是否隐藏。
    pub(crate) fn is_card_hidden(&self, code: &str) -> bool {
        self.data.is_card_hidden(code)
    }

    /// 隐藏卡片代码集合。
    pub(crate) fn hidden_cards(&self) -> Vec<String> {
        self.data.hidden_cards()
    }

    /// 保存卡片完整顺序（落盘）。
    pub(crate) fn set_card_order(&mut self, order: Vec<String>) {
        self.data.set_card_order(order);
    }

    /// 保存隐藏卡片集合（落盘）。
    pub(crate) fn set_hidden_cards(&mut self, hidden: Vec<String>) {
        self.data.set_hidden_cards(hidden);
    }

    /// 右侧栏宽度。
    pub(crate) fn np_width(&self) -> f32 {
        self.data.np_width()
    }

    /// 保存右侧栏宽度（落盘）。
    pub(crate) fn set_np_width(&mut self, width: f32) {
        self.data.set_np_width(width);
    }
}
