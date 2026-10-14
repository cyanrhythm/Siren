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
}
