package subscription

import "time"

// AddCalendarMonths advances anchorSec by n whole months in UTC, truncating the
// day to the last valid day of the target month and preserving the time of day.
// Jan 31 + 1mo → Feb 28 (or 29 in a leap year); Jan 31 + 2mo → Mar 31. This is
// deliberately not time.AddDate, whose overflow normalization would roll Jan 31
// forward into early March.
func AddCalendarMonths(anchorSec int64, n int) int64 {
	t := time.Unix(anchorSec, 0).UTC()

	total := int(t.Month()) - 1 + n
	targetYear := t.Year() + total/12
	targetMonth := total%12 + 1
	if targetMonth <= 0 {
		targetMonth += 12
		targetYear--
	}

	day := t.Day()
	if maxDay := daysInMonth(targetYear, targetMonth); day > maxDay {
		day = maxDay
	}

	return time.Date(targetYear, time.Month(targetMonth), day,
		t.Hour(), t.Minute(), t.Second(), 0, time.UTC).Unix()
}

// daysInMonth returns the number of days in the given 1-based month.
func daysInMonth(year, month int) int {
	return time.Date(year, time.Month(month)+1, 0, 0, 0, 0, 0, time.UTC).Day()
}

// ElapsedCalendarMonths counts whole calendar months elapsed from anchorSec to
// tsSec. A month counts only once its boundary instant has passed; the exact
// boundary belongs to the next period.
func ElapsedCalendarMonths(anchorSec, tsSec int64) int64 {
	if tsSec <= anchorSec {
		return 0
	}

	a := time.Unix(anchorSec, 0).UTC()
	t := time.Unix(tsSec, 0).UTC()

	diff := (t.Year()-a.Year())*12 + (int(t.Month()) - int(a.Month()))
	if diff < 0 {
		return 0
	}
	if AddCalendarMonths(anchorSec, diff) > tsSec {
		diff--
	}
	if diff < 0 {
		diff = 0
	}
	return int64(diff)
}

// ElapsedPeriods returns the 1-based billing period index at nowSec, un-clamped
// (a value above maxPeriods means the subscription window has ended). It returns
// 0 before startAt, and 0 in fixed mode when periodSec is 0.
func ElapsedPeriods(periodMode uint8, startAt, billingAnchorAt, periodSec, nowSec int64) int64 {
	if nowSec < startAt {
		return 0
	}

	if periodMode == PeriodModeCalendarMonth {
		anchor := billingAnchorAt
		if anchor <= 0 {
			anchor = startAt
		}
		startOffset := ElapsedCalendarMonths(anchor, startAt)
		elapsed := ElapsedCalendarMonths(anchor, nowSec)
		v := elapsed - startOffset + 1
		if v < 0 {
			return 0
		}
		return v
	}

	if periodSec == 0 {
		return 0
	}
	return (nowSec-startAt)/periodSec + 1
}
