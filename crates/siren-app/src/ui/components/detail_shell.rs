//! 详情页（歌单 / 专辑 / 歌手）共用的外壳与吸顶横栏。
//!
//! 三页的骨架是同一套：圆角面板外壳 + hero + 收起后的「[播放键][名称]」横栏。
//! 横栏的差异只有「高度怎么变」（折叠式由 fold 收缩，歌单页由外层绝对定位容器控制）、
//! 「底色取不取封面主色」与「播放键何时出现」，故都落在配置字段上。

use gpui_kit::base::StyledExt;
use gpui_kit::*;

use super::buttons::play_circle;
use crate::theme;
use crate::theme::spacing;

/// 详情页外壳：圆角面板 + 纵向排布，撑满中间列。
///
/// ⚠️ 圆角必须落在**画背景的这一层**：gpui 只用矩形内容遮罩裁剪子元素，
/// 父容器（中间列）的圆角裁不掉页面自己画的 `bg(pane)`（见 `page_shell_with_header` 注释）。
pub(crate) fn detail_shell(body: AnyElement) -> AnyElement {
    div()
        .v_flex()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .rounded(px(theme::RADIUS_MD))
        .overflow_hidden()
        .bg(theme::pane())
        .child(body)
        .into_any_element()
}

/// 横栏播放键的回调类型（抽出来只为让 `StickyBar` 的字段类型更好读）。
pub(crate) type PlayHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 吸顶横栏配置：`[播放键][名称]`，底色可带封面主色。
pub(crate) struct StickyBar {
    /// 横栏高度。折叠式页面传 `STICKY_BAR_H * fold`；歌单页把它放在固定高度容器里，传 `STICKY_BAR_H`。
    pub(crate) height: f32,
    /// 整条横栏的不透明度（歌单页用它做淡入；折叠式页面传 1.0，靠高度收缩）。
    pub(crate) opacity: f32,
    /// 底色。`None` = 不画（专辑页在取色就绪前保持透明）。
    pub(crate) bg: Option<Rgba>,
    pub(crate) name: String,
    /// 播放键 id（同一 view 内唯一）。
    pub(crate) play_id: &'static str,
    /// 播放键的不透明度。歌单页让它比整条横栏晚一个阶段出现
    /// （对齐 Melodia 的 `showTitleBar` / `showPlayButton`）；折叠式页面传 1.0。
    pub(crate) play_opacity: f32,
    /// 播放键槽宽：歌单页随出现程度横向展开（48 → 0），折叠式页面固定 48。
    pub(crate) play_width: f32,
    pub(crate) on_play: PlayHandler,
}

impl StickyBar {
    pub(crate) fn new(
        height: f32,
        name: String,
        play_id: &'static str,
        on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            height,
            opacity: 1.0,
            bg: None,
            name,
            play_id,
            play_opacity: 1.0,
            play_width: 48.0,
            on_play: Box::new(on_play),
        }
    }
}

/// 渲染吸顶横栏。默认高度取 `STICKY_BAR_H`（折叠式页面自行乘 fold）。
pub(crate) fn sticky_bar(cfg: StickyBar) -> AnyElement {
    let StickyBar {
        height,
        opacity,
        bg,
        name,
        play_id,
        play_opacity,
        play_width,
        on_play,
    } = cfg;
    let mut bar = div()
        .w_full()
        // 顶部圆角跟着页面外壳走（外壳圆角裁不到子元素自己画的背景）。
        .rounded_t(px(theme::RADIUS_MD))
        .h(px(height))
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(spacing::SM))
        .px(px(spacing::MD))
        .opacity(opacity);
    if let Some(bg) = bg {
        bar = bar.bg(bg);
    }
    bar.child(
        div()
            .w(px(play_width))
            .flex_none()
            .overflow_hidden()
            .opacity(play_opacity)
            .child(play_circle(play_id, 48.0, 22.0, move |ev, window, cx| {
                on_play(ev, window, cx)
            })),
    )
    .child(
        div()
            .flex_1()
            .min_w(px(0.0))
            .text_lg()
            .font_weight(FontWeight::BOLD)
            .text_color(theme::text_primary())
            .truncate()
            .child(name),
    )
    .into_any_element()
}
