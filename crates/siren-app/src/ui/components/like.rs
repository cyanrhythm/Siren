//! 歌曲喜欢按钮：未喜欢时默认隐藏，悬停所在行显示空心，喜欢后常驻红色实心。

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use siren_core::model::Song;

use super::super::icon;
use super::super::playback::Playback;
use crate::theme;

/// 实心红心（liked 常驻态）。
const HEART_FILLED: &[u8] = include_bytes!("../../../../../assets/icons/heart-filled.svg");

/// 行内红心。`group` 需与所在行的 `.group(..)` 名一致，用于悬停显隐。
pub(crate) fn like_button(playback: &Entity<Playback>, song: Song, group: String, cx: &App) -> AnyElement {
    let liked = playback.read(cx).is_liked(song.id);
    let pb = playback.clone();
    let heart: AnyElement = if liked {
        Icon::new(IconName::Heart)
            .data(HEART_FILLED)
            .size(px(16.0))
            .text_color(theme::accent())
            .into_any_element()
    } else {
        icon(IconName::Heart, 16.0, theme::text_gray()).into_any_element()
    };
    let el = div()
        .id(format!("like-{}", song.id))
        .size(px(28.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_click(move |_ev, _window, cx| {
            // 阻止冒泡到行点击（播放）。
            cx.stop_propagation();
            pb.update(cx, |pb, cx| pb.toggle_like_song(song.clone(), cx));
        })
        .child(heart);
    if liked {
        el.into_any_element()
    } else {
        el.opacity(0.0)
            .group_hover(group, |s| s.opacity(1.0))
            .into_any_element()
    }
}
