// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.support;

import com.okx.x402.subscription.model.enums.PeriodMode;
import org.junit.jupiter.api.Test;

import java.time.LocalDateTime;
import java.time.ZoneOffset;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Calendar-month semantics mirrored bit-for-bit from the on-chain Solady DateTimeLib math:
 * anchor never drifts, month-end clamps, the exact boundary instant belongs to the NEXT
 * period, elapsed is uncapped.
 */
class CalendarPeriodMathTest {

    private static final int CAL = PeriodMode.CALENDAR_MONTH.getValue();
    private static final int FIX = PeriodMode.FIXED_SECONDS.getValue();

    private static long utc(int y, int m, int d, int h, int min, int s) {
        return LocalDateTime.of(y, m, d, h, min, s).toEpochSecond(ZoneOffset.UTC);
    }

    // Anchor Jan 31, 12:00 UTC — the month-end torture case.
    private static final long ANCHOR = utc(2026, 1, 31, 12, 0, 0);

    @Test
    void addMonthsClampsToMonthEndWithoutDrift() {
        // +1 clamps to Feb 28; +2 is computed from the ORIGINAL anchor → back to Mar 31.
        assertEquals(utc(2026, 2, 28, 12, 0, 0), PeriodMath.addCalendarMonths(ANCHOR, 1));
        assertEquals(utc(2026, 3, 31, 12, 0, 0), PeriodMath.addCalendarMonths(ANCHOR, 2));
        assertEquals(utc(2026, 4, 30, 12, 0, 0), PeriodMath.addCalendarMonths(ANCHOR, 3));
    }

    @Test
    void elapsedCalendarMonthsBoundaryCorrection() {
        // One second before the clamped Feb boundary → 0 complete months.
        assertEquals(0, PeriodMath.elapsedCalendarMonths(ANCHOR, utc(2026, 2, 28, 11, 59, 59)));
        // The exact boundary instant → 1 complete month.
        assertEquals(1, PeriodMath.elapsedCalendarMonths(ANCHOR, utc(2026, 2, 28, 12, 0, 0)));
        // Before/equal anchor → 0, never negative.
        assertEquals(0, PeriodMath.elapsedCalendarMonths(ANCHOR, ANCHOR));
        assertEquals(0, PeriodMath.elapsedCalendarMonths(ANCHOR, ANCHOR - 1));
    }

    @Test
    void elapsedPeriodsCalendarModeBoundaryBelongsToNextPeriod() {
        // startAt == anchor (plain new subscription): period 1 runs [anchor, +1 month).
        assertEquals(1, PeriodMath.elapsedPeriods(CAL, ANCHOR, ANCHOR, 0, ANCHOR));
        assertEquals(1, PeriodMath.elapsedPeriods(CAL, ANCHOR, ANCHOR, 0, utc(2026, 2, 28, 11, 59, 59)));
        assertEquals(2, PeriodMath.elapsedPeriods(CAL, ANCHOR, ANCHOR, 0, utc(2026, 2, 28, 12, 0, 0)));
        // Before start → period 0.
        assertEquals(0, PeriodMath.elapsedPeriods(CAL, ANCHOR, ANCHOR, 0, ANCHOR - 10));
    }

    @Test
    void inheritedAnchorStartOffsetKeepsPeriodOneBased() {
        // Downgrade activation: new sub starts on the old sub's 3rd boundary but inherits the
        // original anchor. Its first period must still be numbered 1.
        long inheritedStart = PeriodMath.addCalendarMonths(ANCHOR, 3);
        assertEquals(1, PeriodMath.elapsedPeriods(CAL, inheritedStart, ANCHOR, 0, inheritedStart));
        assertEquals(2, PeriodMath.elapsedPeriods(CAL, inheritedStart, ANCHOR, 0,
                PeriodMath.addCalendarMonths(ANCHOR, 4)));
    }

    @Test
    void elapsedPeriodsIsUncapped() {
        // Fixed mode, 12 × 30d sub: 20 periods of wall clock elapsed → 20, not 12.
        long start = 1_000L;
        long periodSec = 86_400L;
        long now = start + 19 * periodSec + 1;
        assertEquals(20, PeriodMath.elapsedPeriods(FIX, start, start, periodSec, now));
        assertEquals(12, PeriodMath.currentPeriod(FIX, start, start, periodSec, 12, now));
    }

    @Test
    void fixedModeZeroPeriodSecIsDefensive() {
        assertEquals(0, PeriodMath.elapsedPeriods(FIX, 1_000L, 1_000L, 0, 2_000L));
    }

    @Test
    void endAtCalendarMode() {
        assertEquals(PeriodMath.addCalendarMonths(ANCHOR, 12),
                PeriodMath.endAt(CAL, ANCHOR, ANCHOR, 0, 12));
    }
}
