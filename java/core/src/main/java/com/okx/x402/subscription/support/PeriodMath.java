// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.support;

import com.okx.x402.subscription.model.enums.PeriodMode;

import java.time.LocalDateTime;
import java.time.ZoneOffset;

/**
 * Period math for x402 subscriptions, mirroring the on-chain A2APaySubscription semantics
 * bit-for-bit for both period modes.
 *
 * <p>FIXED_SECONDS: boundary(n) = startAt + n × periodSec; elapsed = floor((now−startAt)/periodSec)+1.
 *
 * <p>CALENDAR_MONTH (contract uses Solady DateTimeLib, all UTC):
 * <ul>
 *   <li>{@link #addCalendarMonths}: ALWAYS derived from the original anchor (never chained from
 *       the previous boundary), day-of-month clamped to the target month's last day, time-of-day
 *       preserved. Jan 31 12:00 → +1 = Feb 28 12:00 (non-leap year), +2 = Mar 31 12:00 (no drift).</li>
 *   <li>{@link #elapsedCalendarMonths}: calendar year/month component diff, then a single boundary
 *       correction ({@code if addMonths(anchor, diff) > ts then diff−1}) — same algorithm as the
 *       contract.</li>
 * </ul>
 * The exact boundary instant belongs to the NEXT period. All inputs/outputs are unix seconds
 * (UTC). Callers resolve {@code startAt == 0 → now} BEFORE calling, and pass
 * {@code billingAnchorAt} falling back to startAt when the anchor has not been backfilled yet.
 */
public final class PeriodMath {

    private PeriodMath() {}

    /**
     * Add {@code months} calendar months to {@code anchorSec} (UTC), clamping the day-of-month to
     * the target month's last day and preserving the time-of-day. Mirrors Solady
     * DateTimeLib.addMonths (java.time plusMonths computes from the original date, not chained).
     */
    public static long addCalendarMonths(long anchorSec, long months) {
        LocalDateTime anchor = LocalDateTime.ofEpochSecond(anchorSec, 0, ZoneOffset.UTC);
        return anchor.plusMonths(months).toEpochSecond(ZoneOffset.UTC);
    }

    /**
     * Number of COMPLETE calendar months elapsed from {@code anchorSec} to {@code tsSec}.
     * Calendar y/m component diff, then one boundary correction. Returns 0 when
     * {@code tsSec <= anchorSec} (never negative).
     */
    public static long elapsedCalendarMonths(long anchorSec, long tsSec) {
        if (tsSec <= anchorSec) {
            return 0L;
        }
        LocalDateTime anchor = LocalDateTime.ofEpochSecond(anchorSec, 0, ZoneOffset.UTC);
        LocalDateTime ts = LocalDateTime.ofEpochSecond(tsSec, 0, ZoneOffset.UTC);
        long diff = (ts.getYear() - anchor.getYear()) * 12L + (ts.getMonthValue() - anchor.getMonthValue());
        if (diff < 0) {
            return 0L;
        }
        if (addCalendarMonths(anchorSec, diff) > tsSec) {
            diff--;
        }
        return Math.max(diff, 0L);
    }

    /**
     * The n-th period boundary. {@code boundary(0) == startAt} for both modes (calendar mode
     * requires startAt to itself be a boundary of the anchor, which holds by construction: new
     * subs have startAt == anchor; aligned upgrades / downgrade activations start exactly on an
     * old boundary).
     */
    public static long periodBoundaryAt(int periodMode, long startAt, long billingAnchorAt,
                                        long periodSec, long n) {
        if (PeriodMode.isCalendarMonth(periodMode)) {
            long anchor = billingAnchorAt > 0 ? billingAnchorAt : startAt;
            long startOffset = elapsedCalendarMonths(anchor, startAt);
            return addCalendarMonths(anchor, startOffset + n);
        }
        return startAt + n * periodSec;
    }

    /**
     * Raw elapsed period number derived from wall-clock, WITHOUT the maxPeriods cap. Unlike
     * {@link #currentPeriod} (capped, mirrors the contract and drives charge), this keeps growing
     * past maxPeriods so callers can tell an expired subscription apart ("period 20 of 12").
     * 0 before startAt; the exact boundary instant enters the next period; fixed mode with
     * non-positive periodSec returns 0 defensively (corrupt-data guard, avoids div-by-zero).
     *
     * <p>This is the access-gate quantity: access is granted iff
     * {@code lastChargedPeriod >= elapsedPeriods && elapsedPeriods > 0}.
     */
    public static long elapsedPeriods(int periodMode, long startAt, long billingAnchorAt,
                                      long periodSec, long nowSec) {
        if (nowSec < startAt) {
            return 0L;
        }
        if (PeriodMode.isCalendarMonth(periodMode)) {
            long anchor = billingAnchorAt > 0 ? billingAnchorAt : startAt;
            long startOffset = elapsedCalendarMonths(anchor, startAt);
            return elapsedCalendarMonths(anchor, nowSec) - startOffset + 1;
        }
        if (periodSec <= 0) {
            return 0L;
        }
        return (nowSec - startAt) / periodSec + 1;
    }

    /**
     * Current period derived from wall-clock, mirroring the contract's {@code currentPeriod(subId)}:
     * == {@code min(elapsedPeriods(...), maxPeriods)}.
     */
    public static long currentPeriod(int periodMode, long startAt, long billingAnchorAt,
                                     long periodSec, long maxPeriods, long nowSec) {
        return Math.min(elapsedPeriods(periodMode, startAt, billingAnchorAt, periodSec, nowSec), maxPeriods);
    }

    /**
     * Subscription end boundary: {@code periodBoundaryAt(maxPeriods)}. Permit2 allowance
     * expiration and the finalize-expired gate both compare against this instant.
     */
    public static long endAt(int periodMode, long startAt, long billingAnchorAt,
                             long periodSec, long maxPeriods) {
        return periodBoundaryAt(periodMode, startAt, billingAnchorAt, periodSec, maxPeriods);
    }

    // ---- legacy fixed-seconds helpers (pre-periodMode callers) ----

    public static int currentPeriod(long timestamp, long startAt, long periodSec, int maxPeriods) {
        return (int) currentPeriod(PeriodMode.FIXED_SECONDS.getValue(), startAt, startAt,
                periodSec, maxPeriods, timestamp);
    }

    public static boolean serviceEnded(long timestamp, long startAt, long periodSec, int maxPeriods) {
        return timestamp > startAt + (long) maxPeriods * periodSec;
    }

    public static long nextChargeableAt(int currentPeriod, long startAt, long periodSec) {
        return startAt + (long) currentPeriod * periodSec;
    }
}
