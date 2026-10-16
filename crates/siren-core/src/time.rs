//! 本地时间换算。
//!
//! **`chrono` 在本项目的唯一入口**：UI 层不直接依赖 chrono，凡是要把毫秒时间戳
//! 显示成「本地日历日」的地方都走这里。
//!
//! 为什么需要：直接拿 `ms / 86_400_000` 当天数是 **UTC 切日**，东八区 0:00–8:00
//! 的记录会被归到前一天、显示的日期也差一天（整改清单 K5）。时区偏移必须由
//! `chrono::Local` 给出，且**逐时间戳**取——固定用「当前偏移」在夏令时地区会算错。

use chrono::{Datelike, Local, TimeZone};

/// 毫秒时间戳 → 本地日历日 `(年, 月, 日, 星期几)`，星期几 1 = 周一 … 7 = 周日。
///
/// 时间戳超出可表示范围时返回 `None`。
pub fn local_date(ms: i64) -> Option<(i32, u32, u32, u32)> {
    let dt = Local.timestamp_millis_opt(ms).single()?;
    Some((dt.year(), dt.month(), dt.day(), dt.weekday().number_from_monday()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_date_keeps_local_calendar_day() {
        // 用**本地**时间构造凌晨 0:30，再换算回来：必须仍是同一天。
        // 若实现按 UTC 切日，东八区下这里会退到前一天。
        let dt = Local.with_ymd_and_hms(2026, 10, 16, 0, 30, 0).single().unwrap();
        let (y, m, d, _) = local_date(dt.timestamp_millis()).unwrap();
        assert_eq!((y, m, d), (2026, 10, 16), "本地凌晨应仍属当天");
    }

    #[test]
    fn weekday_is_one_based_from_monday() {
        // 2026-10-16 是周五。
        let dt = Local.with_ymd_and_hms(2026, 10, 16, 12, 0, 0).single().unwrap();
        let (_, _, _, weekday) = local_date(dt.timestamp_millis()).unwrap();
        assert_eq!(weekday, 5);
    }

    #[test]
    fn out_of_range_timestamp_is_none() {
        assert!(local_date(i64::MAX).is_none());
    }
}
