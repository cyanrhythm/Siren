//! 「关于应用」页：图标、版本、提交号、许可与简介。
//!
//! 版本取编译期的 `CARGO_PKG_VERSION`；提交号由 `build.rs` 通过 `SIREN_GIT_HASH`
//! 注入（源目录不是 git 仓库时为 `unknown`）。页面无状态，纯展示。

use std::sync::Arc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::Button;
use gpui_kit::*;

use crate::theme::{self, spacing};

/// 应用图标（256×256 PNG，由 `assets/icons/siren.ico` 的 256 帧导出；
/// 应用没有 AssetSource，故走 `img(Arc<Image>)` 这条已有的图片通道）。
const ICON_PNG: &[u8] = include_bytes!("../../../../../assets/icons/siren.png");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("SIREN_GIT_HASH");
const LICENSE: &str = "MIT";
const REPOSITORY: &str = "https://github.com/cyanrhythm/Siren";

const DESCRIPTION: &str = "第三方网易云音乐桌面客户端，使用 Rust + GPUI 编写，\
支持歌单、专辑、歌手、搜索、每日推荐与最近播放，歌词与评论同屏，\
播放能力由 libmpv 提供，界面完全自绘。本项目与网易云音乐官方无关。";

/// 「关于应用」整页（中间栏内容）。
pub(crate) fn about_page() -> AnyElement {
    super::super::page_shell("关于".to_string(), body()).into_any_element()
}

fn body() -> AnyElement {
    let icon = Arc::new(Image::from_bytes(ImageFormat::Png, ICON_PNG.to_vec()));
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.0))
        .p(px(spacing::XL))
        .items_center()
        .child(
            div()
                .v_flex()
                .w_full()
                .max_w(px(560.0))
                .gap(px(spacing::LG))
                .p(px(spacing::XL))
                .rounded(px(theme::RADIUS_LG))
                .bg(theme::card_surface())
                // 顶部：图标 + 名称 + 版本
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(spacing::LG))
                        .child(img(icon).w(px(88.0)).h(px(88.0)).rounded(px(20.0)))
                        .child(
                            div()
                                .v_flex()
                                .min_w(px(0.0))
                                .gap(px(spacing::XXS))
                                .child(div().text_2xl().text_color(theme::text_primary()).child("Siren"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme::text_gray())
                                        .child(format!("版本 {VERSION}")),
                                ),
                        ),
                )
                .child(div().h(px(1.0)).w_full().bg(theme::surface()))
                // 元信息：提交 / 许可 / 仓库
                .child(
                    div()
                        .v_flex()
                        .w_full()
                        .gap(px(spacing::SM))
                        .child(meta_row("提交", GIT_HASH))
                        .child(meta_row("许可", LICENSE))
                        .child(meta_row("仓库", REPOSITORY)),
                )
                .child(
                    div()
                        .text_sm()
                        .line_height(px(22.0))
                        .text_color(theme::text_gray())
                        .child(DESCRIPTION),
                )
                .child(
                    div().flex().gap(px(spacing::SM)).child(
                        Button::new("about-open-repo")
                            .label("打开项目主页")
                            .on_click(|_ev, _window, _cx| siren_platform::open_url(REPOSITORY)),
                    ),
                ),
        )
        .into_any_element()
}

/// 一行「标签 + 值」；标签列固定宽度以便纵向对齐。
fn meta_row(label: &'static str, value: &str) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(px(spacing::MD))
        .child(
            div()
                .w(px(56.0))
                .flex_none()
                .text_sm()
                .text_color(theme::text_gray())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .text_color(theme::text_primary())
                .child(value.to_string()),
        )
        .into_any_element()
}
