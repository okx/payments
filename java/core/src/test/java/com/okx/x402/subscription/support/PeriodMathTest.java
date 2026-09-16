// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.support;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

import static org.junit.jupiter.api.Assertions.*;

class PeriodMathTest {

    private static final long START_AT = 1000L;
    private static final long PERIOD_SEC = 86400L;
    private static final int MAX_PERIODS = 12;

    @Test
    void currentPeriodBeforeStart() {
        assertEquals(0, PeriodMath.currentPeriod(999, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @Test
    void currentPeriodAtStartExactly() {
        assertEquals(1, PeriodMath.currentPeriod(START_AT, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @ParameterizedTest
    @CsvSource({
            "87399, 1",   // startAt + period - 1 -> still period 1
            "87401, 2",   // startAt + period + 1 -> period 2
            "217601, 3",  // startAt + 2.5*period -> period 3
    })
    void currentPeriodMidSubscription(long timestamp, int expected) {
        assertEquals(expected, PeriodMath.currentPeriod(timestamp, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @Test
    void currentPeriodCappedAtMaxPeriods() {
        long wayAfter = START_AT + 100 * PERIOD_SEC;
        assertEquals(MAX_PERIODS, PeriodMath.currentPeriod(wayAfter, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @Test
    void serviceEndedTrue() {
        long afterEnd = START_AT + MAX_PERIODS * PERIOD_SEC + 1;
        assertTrue(PeriodMath.serviceEnded(afterEnd, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @Test
    void serviceEndedFalseAtBoundary() {
        long atBoundary = START_AT + MAX_PERIODS * PERIOD_SEC;
        assertFalse(PeriodMath.serviceEnded(atBoundary, START_AT, PERIOD_SEC, MAX_PERIODS));
    }

    @Test
    void nextChargeableAt() {
        assertEquals(START_AT + 3 * PERIOD_SEC,
                PeriodMath.nextChargeableAt(3, START_AT, PERIOD_SEC));
    }
}
