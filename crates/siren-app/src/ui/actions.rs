//! 全局快捷键（整改清单 O0.4）：动作声明 + 键位表。
//!
//! ## 键位
//!
//! | 键 | 动作 |
//! |---|---|
//! | `Space` | 播放 / 暂停 |
//! | `←` / `→` | 后退 / 前进 5 秒 |
//! | `Ctrl+←` / `Ctrl+→` | 上一首 / 下一首 |
//! | `↑` / `↓` | 音量 +5 / −5 |
//! | `Ctrl+F` | 聚焦标题栏搜索框 |
//! | `PageUp` / `PageDown` | 当前列表上/下翻一页 |
//! | `Home` / `End` | 当前列表到顶 / 到底 |
//!
//! ## 为什么不抢输入框的键
//!
//! gpui 的键位带上下文谓词，gpui-base 的输入框声明 `key_context("Input mode=editor")`，
//! 所以这里统一用 `!Input`：焦点在输入框时这些键照常输入文字（空格、方向键、Home/End 都是）。
//!
//! ## 「当前列表」怎么定
//!
//! 取最近被滚轮/滚动条操作过的滚动区（`smooth_scroll` 里记录）；没有任何操作时退化为
//! 最后一个渲染出的滚动区。见 `smooth_scroll::keyboard_page` / `keyboard_edge`。

use gpui_kit::{actions, App, KeyBinding};

actions!(
    siren,
    [
        TogglePlay,
        NextTrack,
        PrevTrack,
        SeekForward,
        SeekBackward,
        VolumeUp,
        VolumeDown,
        FocusSearch,
        ScrollPageUp,
        ScrollPageDown,
        ScrollTop,
        ScrollBottom,
    ]
);

/// 输入框之外的上下文（gpui-base 的 Input 是 `Input mode=editor`）。
const NOT_INPUT: Option<&str> = Some("!Input");

/// 注册全部全局键位（在 `Root::new` 里调用一次）。
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys(bindings());
}

/// 键位表本身（单测直接用它验证「按键 → 动作」能解析出来）。
pub(crate) fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("space", TogglePlay, NOT_INPUT),
        KeyBinding::new("left", SeekBackward, NOT_INPUT),
        KeyBinding::new("right", SeekForward, NOT_INPUT),
        KeyBinding::new("ctrl-left", PrevTrack, NOT_INPUT),
        KeyBinding::new("ctrl-right", NextTrack, NOT_INPUT),
        KeyBinding::new("up", VolumeUp, NOT_INPUT),
        KeyBinding::new("down", VolumeDown, NOT_INPUT),
        KeyBinding::new("ctrl-f", FocusSearch, NOT_INPUT),
        KeyBinding::new("pageup", ScrollPageUp, NOT_INPUT),
        KeyBinding::new("pagedown", ScrollPageDown, NOT_INPUT),
        KeyBinding::new("home", ScrollTop, NOT_INPUT),
        KeyBinding::new("end", ScrollBottom, NOT_INPUT),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Keymap, Keystroke};

    /// 每个键位都能被 gpui 的键位表解析出来（拼写错、修饰键写错都会在这里挂）。
    #[test]
    fn every_binding_resolves() {
        let keymap = Keymap::new(bindings());
        for (stroke, label) in [
            ("space", "播放/暂停"),
            ("left", "后退 5s"),
            ("right", "前进 5s"),
            ("ctrl-left", "上一首"),
            ("ctrl-right", "下一首"),
            ("up", "音量 +"),
            ("down", "音量 −"),
            ("ctrl-f", "聚焦搜索"),
            ("pageup", "上翻页"),
            ("pagedown", "下翻页"),
            ("home", "到顶"),
            ("end", "到底"),
        ] {
            let keystroke =
                Keystroke::parse(stroke).unwrap_or_else(|err| panic!("{label} 的键位 {stroke} 解析失败：{err}"));
            let matched = keymap.all_bindings_for_input(&[keystroke]);
            assert!(!matched.is_empty(), "{label}（{stroke}）没有解析出任何动作");
        }
    }

    /// 输入框上下文里的按键**不能**被这些绑定抢走（谓词 `!Input` 的回归用例）。
    #[test]
    fn bindings_are_excluded_in_input_context() {
        let keymap = Keymap::new(bindings());
        let stroke = Keystroke::parse("space").unwrap();
        let matched = keymap.all_bindings_for_input(&[stroke]);
        let binding = matched.first().expect("space 应有绑定");
        let predicate = binding.predicate().expect("应带上下文谓词");
        let context: Vec<gpui_kit::KeyContext> =
            vec![gpui_kit::KeyContext::parse("Input mode=editor").expect("上下文解析")];
        assert!(!predicate.depth_of(&context).is_some(), "输入框里不应命中全局快捷键");
    }
}
