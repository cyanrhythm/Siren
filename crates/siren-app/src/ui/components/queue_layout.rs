//! 播放队列的纯布局常量与下标换算。
//!
//! 与 GPUI 解耦（不引入 `gpui_kit`），便于单元测试；渲染在 [`super::queue_panel`]。

/// 队列行高（拖动排序按此换算目标下标）。
pub(super) const ROW_H: f32 = 52.0;
/// 分段标题行高。
pub(super) const SECTION_H: f32 = 28.0;
/// 列表内元素间距。
pub(super) const GAP: f32 = 2.0;
/// 列表内边距。
pub(super) const PAD: f32 = 8.0;
/// 相邻行的槽位步长（FLIP 位移按此换算）。
pub(super) const STRIDE: f32 = ROW_H + GAP;

/// 由指针在列表视口中的 y（相对滚动容器顶）推算「接下来播放」内的目标槽位。
///
/// `scroll_offset` 为 `ScrollHandle::offset().y`（滚下时为负），需先还原到内容坐标系，
/// 否则长「已播放」段滚动后拖动会因偏移而始终落到首行。
pub(super) fn reorder_target(pointer_y: f32, scroll_offset: f32, top: f32, upcoming_start: usize, n: usize) -> usize {
    if upcoming_start >= n {
        return upcoming_start;
    }
    let content_y = pointer_y - scroll_offset;
    let relative = (content_y - top).max(0.0);
    ((relative / STRIDE).floor() as usize + upcoming_start).min(n - 1)
}

/// 队列第 `index` 行在列表内容坐标系中的顶偏移（含分段标题、列表间距与内边距）。
///
/// 与 `queue_panel` 的渲染布局逐项对应；越界返回 `None`。
/// 供「打开浮层定位到当前曲」换算滚动目标。
pub(super) fn row_content_top(index: usize, current: Option<usize>, n: usize) -> Option<f32> {
    if index >= n {
        return None;
    }
    let current = current.filter(|c| *c < n);
    let played_end = current.unwrap_or(0);
    let has_played = current.is_some() && played_end > 0;
    // 已播放段占的 kid 数（无已播放段时为 0）；目标行的 kid 序与其之前全部 kid 的高度和。
    let played_kids = if has_played { 1 + played_end } else { 0 };
    let (kid, height) = if has_played && index < played_end {
        (1 + index, SECTION_H + index as f32 * ROW_H)
    } else if Some(index) == current {
        let played = if has_played {
            SECTION_H + played_end as f32 * ROW_H
        } else {
            0.0
        };
        (played_kids + 1, played + SECTION_H)
    } else {
        let played = if has_played {
            SECTION_H + played_end as f32 * ROW_H
        } else {
            0.0
        };
        let current_rows = if current.is_some() { SECTION_H + ROW_H } else { 0.0 };
        let upcoming_start = current.map(|c| c + 1).unwrap_or(0);
        let rel = index - upcoming_start;
        (
            played_kids + if current.is_some() { 2 } else { 0 } + 1 + rel,
            played + current_rows + SECTION_H + rel as f32 * ROW_H,
        )
    };
    Some(PAD + height + GAP * kid as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 首个「接下来播放」行的内容顶偏移（前有已播放段 + 正在播放段）。
    fn upcoming_top(played: usize) -> f32 {
        PAD + (SECTION_H + played as f32 * ROW_H) + (SECTION_H + ROW_H) + SECTION_H + GAP * 13.0
    }

    #[test]
    fn reorder_target_tracks_pointer_without_scroll() {
        let top = upcoming_top(10);
        let (start, n) = (11, 20);
        assert_eq!(reorder_target(top + 5.0, 0.0, top, start, n), start);
        assert_eq!(reorder_target(top + STRIDE + 5.0, 0.0, top, start, n), start + 1);
    }

    #[test]
    fn reorder_target_compensates_scroll_offset() {
        let top = upcoming_top(10);
        let (start, n) = (11, 20);
        // 视口滚下一行后，同一屏幕位置对应内容坐标的下一槽位。
        assert_eq!(reorder_target(top + 5.0, -ROW_H, top, start, n), start + 1);
        // 指针在列表顶（负相对位置）钳到首行，不越界。
        assert_eq!(reorder_target(0.0, -ROW_H, top, start, n), start);
    }

    #[test]
    fn reorder_target_clamps_to_tail() {
        let top = PAD + SECTION_H;
        assert_eq!(reorder_target(top + 100_000.0, 0.0, top, 1, 4), 3);
    }

    #[test]
    fn row_content_top_matches_render_layout() {
        // 当前曲 = 10、共 20 首：正在播放行之前是「已播放」段（标题 + 10 行）+ 正在播放标题。
        let current = row_content_top(10, Some(10), 20).unwrap();
        assert_eq!(current, PAD + (SECTION_H + 10.0 * ROW_H) + SECTION_H + GAP * 12.0);
        // 首个「接下来播放」行：正在播放行后再隔「接下来播放」标题（其前有 14 个 kid，即 14 个间距）。
        assert_eq!(
            row_content_top(11, Some(10), 20).unwrap(),
            PAD + (SECTION_H + 10.0 * ROW_H) + (SECTION_H + ROW_H) + SECTION_H + GAP * 14.0
        );
        // 无当前曲时列表只有「接下来播放」段。
        assert_eq!(row_content_top(0, None, 3).unwrap(), PAD + SECTION_H + GAP * 1.0);
        // 当前曲在首位（无已播放段）：当前行是第 1 个 kid。
        assert_eq!(row_content_top(0, Some(0), 3).unwrap(), PAD + SECTION_H + GAP * 1.0);
        // 已播放段末行（第 10 个 kid）。
        assert_eq!(
            row_content_top(9, Some(10), 20).unwrap(),
            PAD + SECTION_H + 9.0 * ROW_H + GAP * 10.0
        );
        // 越界返回 None。
        assert!(row_content_top(20, Some(10), 20).is_none());
    }
}
