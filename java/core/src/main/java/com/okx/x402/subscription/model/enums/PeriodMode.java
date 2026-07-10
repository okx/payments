// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

/**
 * Billing period mode (17th SubscriptionTerms field).
 *
 * <p>FIXED_SECONDS: boundary(n) = startAt + n × periodSec (periodSec must be &gt; 0).
 * CALENDAR_MONTH: boundary(n) = addMonths(billingAnchorAt, offset + n) (periodSec must be 0).
 */
public enum PeriodMode {
    FIXED_SECONDS(0),
    CALENDAR_MONTH(1);

    private final int value;

    PeriodMode(int value) {
        this.value = value;
    }

    public int getValue() {
        return value;
    }

    public static boolean isCalendarMonth(int code) {
        return code == CALENDAR_MONTH.value;
    }
}
