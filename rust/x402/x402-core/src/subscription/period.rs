//! Period math for the `period` scheme.
//!
//! Computes the buyer's current period number locally to gate access without a
//! facilitator round-trip. Must match the facilitator/contract accounting:
//! - every calendar boundary is `addMonths(anchor, n)` from the original anchor,
//!   month-end truncated (1/31 → 2/28 → 3/31), preserving time-of-day; the exact
//!   boundary instant belongs to the next period;
//! - all timestamps are UTC.
//!
//! Results are un-clamped: a value `> maxPeriods` means the service window ended.

use chrono::{Datelike, Months, TimeZone, Utc};

/// Fixed-seconds mode: period length is `periodSec` (`periodSec > 0`).
pub const PERIOD_MODE_FIXED: u8 = 0;
/// Calendar-month mode: calendar months from the anchor (`periodSec == 0`).
pub const PERIOD_MODE_CALENDAR_MONTH: u8 = 1;

/// Add `n` calendar months to `anchor_sec` (UTC), truncating to the last valid
/// day of the target month (1/31 + 1mo → 2/28) and preserving the time-of-day.
/// Returns the anchor unchanged on an out-of-range result.
pub fn add_calendar_months(anchor_sec: i64, n: u32) -> i64 {
    match Utc.timestamp_opt(anchor_sec, 0).single() {
        Some(dt) => dt
            .checked_add_months(Months::new(n))
            .map(|d| d.timestamp())
            .unwrap_or(anchor_sec),
        None => anchor_sec,
    }
}

/// Whole calendar months elapsed from `anchor_sec` to `ts_sec` (UTC), counting a
/// month only once its boundary instant has passed.
fn elapsed_calendar_months(anchor_sec: i64, ts_sec: i64) -> i64 {
    if ts_sec <= anchor_sec {
        return 0;
    }
    let (anchor, ts) = match (
        Utc.timestamp_opt(anchor_sec, 0).single(),
        Utc.timestamp_opt(ts_sec, 0).single(),
    ) {
        (Some(a), Some(t)) => (a, t),
        _ => return 0,
    };
    let mut diff = (ts.year() as i64 - anchor.year() as i64) * 12
        + (ts.month() as i64 - anchor.month() as i64);
    if diff < 0 {
        return 0;
    }
    // The year/month diff over-counts when `ts` has not yet reached the
    // day/time of the `diff`-th boundary; step back one month.
    if add_calendar_months(anchor_sec, diff as u32) > ts_sec {
        diff -= 1;
    }
    diff.max(0)
}

/// The buyer's current 1-based, un-clamped period number at `now_sec`.
///
/// - `0` before start (`now < startAt`) or when the mode's required field is
///   missing (fixed mode with `periodSec == 0`);
/// - fixed mode: `(now - startAt) / periodSec + 1`;
/// - calendar mode: `elapsedCalendarMonths(anchor, now) - elapsedCalendarMonths(anchor, startAt) + 1`,
///   where `anchor = billingAnchorAt > 0 ? billingAnchorAt : startAt`.
///
/// A value `> maxPeriods` means the service window has ended.
pub fn elapsed_periods(
    period_mode: u8,
    start_at: u64,
    billing_anchor_at: u64,
    period_sec: u64,
    now_sec: u64,
) -> u64 {
    if now_sec < start_at {
        return 0;
    }
    if period_mode == PERIOD_MODE_CALENDAR_MONTH {
        let anchor = if billing_anchor_at > 0 {
            billing_anchor_at
        } else {
            start_at
        };
        let start_offset = elapsed_calendar_months(anchor as i64, start_at as i64);
        let elapsed = elapsed_calendar_months(anchor as i64, now_sec as i64);
        return (elapsed - start_offset + 1).max(0) as u64;
    }
    if period_sec == 0 {
        return 0;
    }
    (now_sec - start_at) / period_sec + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2021-01-31 12:00:00 UTC
    const JAN31_NOON: i64 = 1_612_094_400;

    fn ymd_hms(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap().timestamp()
    }

    #[test]
    fn add_months_truncates_month_end() {
        // 1/31 + 1mo → 2/28 (non-leap 2021), preserving 12:00:00.
        assert_eq!(add_calendar_months(JAN31_NOON, 1), ymd_hms(2021, 2, 28, 12, 0, 0));
        // + 2mo → 3/31 (no drift to the 28th rhythm).
        assert_eq!(add_calendar_months(JAN31_NOON, 2), ymd_hms(2021, 3, 31, 12, 0, 0));
        // + 3mo → 4/30.
        assert_eq!(add_calendar_months(JAN31_NOON, 3), ymd_hms(2021, 4, 30, 12, 0, 0));
    }

    #[test]
    fn add_months_leap_year_feb29() {
        // 2020 is a leap year: 1/31 + 1mo → 2/29.
        let jan31_2020 = ymd_hms(2020, 1, 31, 0, 0, 0);
        assert_eq!(add_calendar_months(jan31_2020, 1), ymd_hms(2020, 2, 29, 0, 0, 0));
    }

    #[test]
    fn add_months_crosses_year() {
        // 2021-12-15 + 1mo → 2022-01-15.
        let dec = ymd_hms(2021, 12, 15, 8, 30, 0);
        assert_eq!(add_calendar_months(dec, 1), ymd_hms(2022, 1, 15, 8, 30, 0));
    }

    #[test]
    fn elapsed_periods_zero_before_start() {
        // now < startAt → 0 (fixed and calendar).
        assert_eq!(elapsed_periods(PERIOD_MODE_FIXED, 1000, 0, 86_400, 999), 0);
        assert_eq!(elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, 1000, 0, 0, 999), 0);
    }

    #[test]
    fn elapsed_periods_fixed_seconds() {
        let start = 1_000_000u64;
        let period = 2_592_000u64; // 30 days
        // exactly at start → period 1.
        assert_eq!(elapsed_periods(PERIOD_MODE_FIXED, start, 0, period, start), 1);
        // 1s before the 2nd boundary → still period 1.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_FIXED, start, 0, period, start + period - 1),
            1
        );
        // exactly at the 2nd boundary → period 2.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_FIXED, start, 0, period, start + period),
            2
        );
        // deep into period 4.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_FIXED, start, 0, period, start + period * 3 + 5),
            4
        );
    }

    #[test]
    fn elapsed_periods_fixed_zero_period_sec_is_zero() {
        // fixed mode requires periodSec > 0; 0 is invalid → 0.
        assert_eq!(elapsed_periods(PERIOD_MODE_FIXED, 1000, 0, 0, 5000), 0);
    }

    #[test]
    fn elapsed_periods_calendar_anchor_eq_start() {
        // anchor = start = 2021-01-31 12:00. now at start → period 1.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, JAN31_NOON as u64, 0, 0, JAN31_NOON as u64),
            1
        );
        // 1s before the 2/28 boundary → still period 1.
        let feb28 = ymd_hms(2021, 2, 28, 12, 0, 0) as u64;
        assert_eq!(
            elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, JAN31_NOON as u64, 0, 0, feb28 - 1),
            1
        );
        // exactly at 2/28 12:00 boundary → period 2.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, JAN31_NOON as u64, 0, 0, feb28),
            2
        );
        // 3/31 12:00 → period 3.
        let mar31 = ymd_hms(2021, 3, 31, 12, 0, 0) as u64;
        assert_eq!(
            elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, JAN31_NOON as u64, 0, 0, mar31),
            3
        );
    }

    #[test]
    fn elapsed_periods_calendar_anchor_before_start() {
        // Aligned-upgrade case: anchor (old sub) precedes startAt (new sub).
        // anchor = 2021-01-15, start = 2021-03-15, now = 2021-04-20.
        let anchor = ymd_hms(2021, 1, 15, 0, 0, 0) as u64;
        let start = ymd_hms(2021, 3, 15, 0, 0, 0) as u64;
        let now = ymd_hms(2021, 4, 20, 0, 0, 0) as u64;
        // startOffset = 2 (Jan→Mar), elapsed(now) = 3 (Jan→Apr), 3 - 2 + 1 = 2.
        assert_eq!(
            elapsed_periods(PERIOD_MODE_CALENDAR_MONTH, start, anchor, 0, now),
            2
        );
    }

    #[test]
    fn elapsed_calendar_months_basic() {
        let anchor = ymd_hms(2021, 1, 31, 12, 0, 0);
        // before the 2/28 boundary → 0 whole months.
        assert_eq!(elapsed_calendar_months(anchor, ymd_hms(2021, 2, 27, 0, 0, 0)), 0);
        // at/after 2/28 12:00 → 1.
        assert_eq!(elapsed_calendar_months(anchor, ymd_hms(2021, 2, 28, 12, 0, 0)), 1);
        // ts <= anchor → 0.
        assert_eq!(elapsed_calendar_months(anchor, anchor), 0);
        assert_eq!(elapsed_calendar_months(anchor, anchor - 1), 0);
    }
}
