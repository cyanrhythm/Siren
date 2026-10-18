//! 设置页（逐步补齐）。
//!
//! 目前落地「音质」档位选择；其余分节按 `docs/08-功能对照与清单.md` 的里程碑补齐。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use siren_core::model::AudioQuality;

use super::super::settings_store::SettingsStore;
use super::super::smooth_scroll::smooth_scroll;
use super::super::{icon, motion, page_shell, Root};
use crate::theme::{self, spacing};

/// 正在播放卡片编辑器行高。
const CARD_ROW_H: f32 = 44.0;

/// 音质 pill 水平内边距（非 spacing 档位的固定尺寸）。
const PILL_PAD_X: f32 = 12.0;

/// 拖动拾起时跟随光标的浮动预览（拾起入场：淡入 + 轻微上浮）。
struct CardDragPreview {
    label: String,
}

impl Render for CardDragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(CARD_ROW_H))
            .px(px(14.0))
            .flex()
            .items_center()
            .gap(px(spacing::SM))
            .rounded(px(theme::RADIUS_MD))
            .bg(theme::card_surface())
            .shadow_lg()
            .child(icon(IconName::GripVertical, 18.0, theme::text_gray()))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(self.label.clone()),
            )
            .with_animation(
                "card-drag-preview",
                Animation::new(motion::DRAG_ENTER).with_easing(motion::ease_out()),
                |el, delta| el.opacity(delta).top(px(-8.0 * (1.0 - delta))),
            )
    }
}

/// 卡片编辑器当前顺序的稳定键（用于重排归位补间）。
fn card_keys(root: &Root, cx: &App) -> Vec<String> {
    root.editor_layout(cx)
        .iter()
        .map(|&(card, _)| format!("c:{}", card.code()))
        .collect()
}

impl Root {
    pub(crate) fn settings_page(
        &self,
        weak: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.playback.read(cx).quality(cx);
        let media_on = self.playback.read(cx).media_keys_enabled(cx);
        let audio_cache_on = self.playback.read(cx).audio_cache_enabled(cx);
        let audio_cache_control = {
            let weak_toggle = weak.clone();
            Switch::new("audio-cache")
                .checked(audio_cache_on)
                .on_click(move |checked, _window, cx| {
                    weak_toggle
                        .update(cx, |root, cx| {
                            root.playback
                                .update(cx, |pb, cx| pb.set_audio_cache_enabled(*checked, cx));
                        })
                        .ok();
                })
                .into_any_element()
        };

        let weak_media = weak.clone();
        let media_control = Switch::new("media-keys")
            .checked(media_on)
            .on_click(move |checked, _window, cx| {
                weak_media
                    .update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.set_media_keys(*checked, cx));
                    })
                    .ok();
            })
            .into_any_element();

        let weak_tray = weak.clone();
        let tray_control = Switch::new("close-to-tray")
            .checked(self.close_to_tray_setting(cx))
            .on_click(move |checked, _window, cx| {
                weak_tray
                    .update(cx, |root, cx| root.set_close_to_tray(*checked, cx))
                    .ok();
            })
            .into_any_element();

        let weak_np_scrollbar = weak.clone();
        let np_scrollbar_control = Switch::new("np-scrollbar")
            .checked(SettingsStore::of(cx).np_scrollbar())
            .on_click(move |checked, _window, cx| {
                weak_np_scrollbar
                    .update(cx, |root, cx| root.set_np_scrollbar(*checked, cx))
                    .ok();
            })
            .into_any_element();

        let content = div()
            .v_flex()
            .w_full()
            .gap(px(spacing::LG))
            .p(px(spacing::LG))
            .child(section(
                "音质",
                "在线播放的音质档位（更高档位需登录 / 会员）",
                quality_row(current, weak.clone()),
            ))
            .child(section(
                "系统",
                "系统媒体控制与关闭行为",
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(toggle_row("系统媒体控制（媒体键与系统播放卡片）", media_control))
                    .child(toggle_row("关闭主窗口时最小化到托盘（从托盘菜单退出）", tray_control))
                    .into_any_element(),
            ))
            .child(section(
                "右侧栏",
                "正在播放栏宽度与滚动条",
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(self.width_control(cx))
                    .child(toggle_row("歌曲详情页显示滚动条", np_scrollbar_control))
                    .into_any_element(),
            ))
            .child(section(
                "正在播放卡片",
                "右侧面板卡片的顺序与显隐",
                self.cards_control(weak.clone(), cx),
            ))
            .child(section(
                "图片缓存",
                "封面与头像：内存上限 64 MB（按最近使用淘汰）；磁盘 500 MB（命中时不再下载）",
                self.cache_control(weak.clone()),
            ))
            .child(section(
                "音频缓存",
                "播放过的歌曲落盘，再次播放直接读本地（也支持断网播放）；关闭后只流式播放",
                div()
                    .v_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(toggle_row("播放时缓存到本地", audio_cache_control))
                    .child(self.audio_cache_control(weak.clone(), cx))
                    .into_any_element(),
            ));

        let scroll = smooth_scroll("settings-scroll", content, window, cx);

        page_shell("设置".to_string(), scroll.into_any_element()).into_any_element()
    }

    /// 图片缓存占用与清除入口。
    fn cache_control(&self, weak: WeakEntity<Self>) -> AnyElement {
        let stats = self.services.covers.stats();
        let rss = siren_platform::rss_bytes()
            .map(|bytes| format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)))
            .unwrap_or_else(|| "—".to_string());
        let (disk_mb, disk_files) = crate::disk_lru::summary(stats.disk);
        let usage = format!(
            "{} 张 · 约 {:.1} MB（压缩 {:.1} MB + 解码 {:.1} MB）· 进程 RSS {}",
            stats.entries,
            stats.total_mb(),
            stats.compressed_bytes as f64 / (1024.0 * 1024.0),
            stats.decoded_bytes as f64 / (1024.0 * 1024.0),
            rss
        );
        let disk = format!(
            "磁盘 {disk_files} 个文件 · {disk_mb:.1} MB / {} MB",
            super::super::cover_disk::COVER_DISK_LIMIT / (1024 * 1024)
        );
        let weak_clear = weak;
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::MD))
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(spacing::XXS))
                    .child(div().text_sm().text_color(theme::text_primary()).child(usage))
                    .child(div().text_xs().text_color(theme::text_gray()).child(disk)),
            )
            .child(
                Button::new("clear-cover-cache")
                    .label("清除图片缓存")
                    .on_click(move |_ev, _window, cx| {
                        weak_clear
                            .update(cx, |root, cx| {
                                root.services.covers.clear(cx);
                                cx.notify();
                            })
                            .ok();
                    }),
            )
            .into_any_element()
    }

    /// 音频缓存占用与清除入口（开关在调用方那行）。
    fn audio_cache_control(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let playback = self.playback.read(cx);
        let stats = playback.audio_cache_stats();
        let limit = playback.audio_cache_limit();
        let (mb, files) = crate::disk_lru::summary(stats);
        let text = if limit == 0 {
            "本地缓存不可用（取不到缓存目录）".to_string()
        } else {
            format!("已缓存 {files} 首 · {mb:.1} MB / {} MB", limit / (1024 * 1024))
        };
        let weak_clear = weak;
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::MD))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_xs()
                    .text_color(theme::text_gray())
                    .child(text),
            )
            .child(
                Button::new("clear-audio-cache")
                    .label("清除音频缓存")
                    .on_click(move |_ev, _window, cx| {
                        weak_clear
                            .update(cx, |root, cx| {
                                root.playback.update(cx, |pb, cx| pb.clear_audio_cache(cx));
                                cx.notify();
                            })
                            .ok();
                    }),
            )
            .into_any_element()
    }

    /// 关闭到托盘设置当前值。
    fn close_to_tray_setting(&self, cx: &App) -> bool {
        SettingsStore::of(cx).close_to_tray()
    }

    /// 正在播放卡片：拖动把手调整顺序，开关控制显隐（对齐 Melodia `CardLayoutEditor`）。
    fn cards_control(&self, weak: WeakEntity<Self>, cx: &App) -> AnyElement {
        let layout = self.editor_layout(cx);

        let mut rows = div().v_flex().w_full();
        for (i, (card, visible)) in layout.iter().enumerate() {
            let card = *card;
            let visible = *visible;
            let key = format!("c:{}", card.code());
            let flip = self.flip_of(&key);
            let weak_handle = weak.clone();
            let weak_switch = weak.clone();
            let row = div()
                .w_full()
                .h(px(CARD_ROW_H))
                .flex()
                .items_center()
                .gap(px(spacing::SM))
                .child(
                    div()
                        .id(("card-drag", i))
                        .size(px(28.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .on_drag(i, move |_idx, _offset, _window, cx| {
                            let label = card.label().to_string();
                            weak_handle
                                .update(cx, |root, _cx| {
                                    root.clear_drag_flip();
                                    root.set_card_drag_from(Some(i));
                                })
                                .ok();
                            cx.new(|_| CardDragPreview { label: label.clone() })
                        })
                        .child(icon(IconName::GripVertical, 18.0, theme::text_gray())),
                )
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(theme::text_primary())
                        .child(card.label()),
                )
                .child(
                    Switch::new(("card-switch", i))
                        .checked(visible)
                        .on_click(move |checked, _window, cx| {
                            weak_switch
                                .update(cx, |root, cx| root.set_card_visible(card, *checked, cx))
                                .ok();
                        }),
                );
            // 重排归位：从旧槽位偏移滑回 0（屏幕内移动 → 强 ease-in-out）。
            let row = match flip {
                Some((from, generation)) if from != 0.0 => row
                    .with_animation(
                        ElementId::Name(format!("card-flip-{key}-{generation}").into()),
                        Animation::new(motion::REORDER).with_easing(motion::ease_in_out()),
                        move |el, delta| el.top(px(from * (1.0 - delta))),
                    )
                    .into_any_element(),
                _ => row.into_any_element(),
            };
            rows = rows.child(row);
        }

        div()
            .v_flex()
            .w_full()
            .on_drag_move::<usize>({
                let weak = weak.clone();
                move |event: &DragMoveEvent<usize>, _window, cx| {
                    let y = f32::from(event.event.position.y - event.bounds.origin.y);
                    let target = (y / CARD_ROW_H).floor().max(0.0) as usize;
                    weak.update(cx, |root, cx| {
                        let Some(from) = root.card_drag_from() else {
                            return;
                        };
                        let n = root.editor_layout(cx).len();
                        if n == 0 {
                            return;
                        }
                        let to = target.min(n - 1);
                        if to != from {
                            let old_keys = card_keys(root, cx);
                            root.reorder_card(from, to, cx);
                            let new_keys = card_keys(root, cx);
                            let drag_key = new_keys.get(to).cloned();
                            root.flip_on_reorder(&old_keys, &new_keys, CARD_ROW_H, drag_key.as_deref());
                            root.set_card_drag_from(Some(to));
                        }
                    })
                    .ok();
                }
            })
            .child(rows)
            .into_any_element()
    }

    /// 右侧栏宽度滑块。
    fn width_control(&self, cx: &App) -> AnyElement {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(spacing::MD))
            .child(div().w(px(240.0)).child(Slider::new(self.np_width_slider())))
            .child(
                div()
                    .text_sm()
                    .text_color(theme::text_primary())
                    .child(format!("{} px", self.np_width(cx) as i32)),
            )
            .into_any_element()
    }
}

/// 一个设置分节：标题 + 说明 + 控件。
fn section(title: &str, subtitle: &str, control: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .w_full()
        .gap(px(spacing::SM))
        .p(px(14.0))
        .rounded(px(theme::RADIUS_MD))
        .bg(theme::card_surface())
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(title.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme::text_gray())
                .child(subtitle.to_string()),
        )
        .child(control)
        .into_any_element()
}

/// 一行开关：左标签 + 右控件。
fn toggle_row(label: &str, control: AnyElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(spacing::MD))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .text_color(theme::text_primary())
                .child(label.to_string()),
        )
        .child(control)
        .into_any_element()
}

/// 音质档位选择（pills）。
fn quality_row(current: AudioQuality, weak: WeakEntity<Root>) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap(px(spacing::SM));
    for quality in AudioQuality::ALL {
        let active = quality == current;
        let weak = weak.clone();
        row = row.child(
            div()
                .id(("quality", quality as usize))
                .px(px(PILL_PAD_X))
                .py(px(spacing::XXS))
                .rounded_full()
                .cursor_pointer()
                // 选中态统一：surface_light 背景（白字），hover 只在非选中时生效。
                .bg(if active {
                    theme::surface_light()
                } else {
                    theme::pane()
                })
                .when(!active, |s| s.hover(|s| s.bg(theme::surface_light())))
                .on_click(move |_ev, _window, cx| {
                    weak.update(cx, |root, cx| {
                        root.playback.update(cx, |pb, cx| pb.set_quality(quality, cx));
                    })
                    .ok();
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(spacing::XS))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_primary())
                                .child(quality.display_name()),
                        )
                        .when(active, |s| s.child(icon(IconName::Check, 14.0, theme::text_primary()))),
                ),
        );
    }
    row.into_any_element()
}
