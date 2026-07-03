/**
 * Local mirror of the facilitator's elapsed-period math. Used by
 * `verifyAccess` to decide whether `lastChargedPeriod` is current without
 * hitting the facilitator on every request — only when the local
 * computation says we're behind does the SDK fall through to a GET /detail
 * refresh.
 *
 * MUST stay bit-for-bit equivalent to the backend implementation. Returns
 * `0` when the subscription has not yet started — caller treats this as a
 * special "pre-start" state.
 */

const PERIOD_MODE_CALENDAR_MONTH = 1;

/**
 * Compute the real elapsed period count at `nowSec`, NOT clamped to
 * `maxPeriods`. Period 1 is the first chargeable period; `0` means
 * pre-start (subscription not yet effective).
 */
export function computeElapsedPeriods(
  periodMode: number,
  startAt: number,
  billingAnchorAt: number,
  periodSec: number,
  nowSec: number,
): number {
  if (nowSec < startAt) return 0;
  if (periodMode === PERIOD_MODE_CALENDAR_MONTH) {
    const anchor = billingAnchorAt > 0 ? billingAnchorAt : startAt;
    const startOffset = elapsedCalendarMonths(anchor, startAt);
    return elapsedCalendarMonths(anchor, nowSec) - startOffset + 1;
  }
  if (periodSec <= 0) return 0;
  return Math.floor((nowSec - startAt) / periodSec) + 1;
}

/**
 * Number of calendar-month boundaries crossed between `anchorSec` (exclusive)
 * and `tsSec` (inclusive). Each boundary is `addCalendarMonths(anchor, n)`
 * for n=1,2,…, with month-end truncation (1/31 + 1m → 2/28/29).
 */
export function elapsedCalendarMonths(anchorSec: number, tsSec: number): number {
  if (tsSec <= anchorSec) return 0;
  const anchor = new Date(anchorSec * 1000);
  const ts = new Date(tsSec * 1000);
  let diff =
    (ts.getUTCFullYear() - anchor.getUTCFullYear()) * 12 +
    (ts.getUTCMonth() - anchor.getUTCMonth());
  if (diff < 0) return 0;
  if (addCalendarMonths(anchorSec, diff) > tsSec) diff--;
  return Math.max(diff, 0);
}

/**
 * Add `n` calendar months to `anchorSec`, keeping the anchor's day-of-month
 * but truncating to month-end when overflowing (1/31 + 1m → 2/28 or 2/29).
 * Returned timestamp preserves the anchor's hour / minute / second / ms in UTC.
 */
export function addCalendarMonths(anchorSec: number, n: number): number {
  const anchor = new Date(anchorSec * 1000);
  const targetYear = anchor.getUTCFullYear() + Math.floor((anchor.getUTCMonth() + n) / 12);
  const targetMonth = (((anchor.getUTCMonth() + n) % 12) + 12) % 12;
  // Days in target month (UTC) — day 0 of month+1 is the last day of month.
  const daysInTargetMonth = new Date(Date.UTC(targetYear, targetMonth + 1, 0)).getUTCDate();
  const day = Math.min(anchor.getUTCDate(), daysInTargetMonth);
  const ts = Date.UTC(
    targetYear,
    targetMonth,
    day,
    anchor.getUTCHours(),
    anchor.getUTCMinutes(),
    anchor.getUTCSeconds(),
    anchor.getUTCMilliseconds(),
  );
  return Math.floor(ts / 1000);
}
