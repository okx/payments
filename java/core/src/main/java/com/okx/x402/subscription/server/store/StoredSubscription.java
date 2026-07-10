// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.store;

import com.okx.x402.subscription.model.enums.PeriodMode;
import com.okx.x402.subscription.model.resp.QueryResp;

public class StoredSubscription {
    public String subId;
    public int state;
    public String payer;
    public String merchant;
    public String token;
    public String amountPerPeriod;
    public long periodSec;
    /** 0 = FIXED_SECONDS / 1 = CALENDAR_MONTH. */
    public int periodMode;
    public int maxPeriods;
    /**
     * Effective start (unix sec). 0 means the buyer signed startAt=0 (contract uses block time)
     * and the real value has not been refreshed from /subscriptions/detail yet — access checks
     * MUST refresh before doing local period math.
     */
    public long startAt;
    /** Calendar-month billing anchor; 0 = not yet backfilled (refresh from detail). */
    public long billingAnchorAt;
    /**
     * Highest period actually charged (on-chain fact). Access gate: grant iff
     * lastChargedPeriod >= elapsedPeriods && elapsedPeriods > 0. Three-state:
     * null = never fetched (forces the access slow path); 0 = known, never charged.
     */
    public Long lastChargedPeriod;
    /** Next chargeable boundary (unix sec) — drives dueSubscriptions; null until known. */
    public Long nextChargeableAt;
    /** When this record was last written (unix sec) — drives the access-cache TTL. */
    public long updatedAt;
    public int planTier;
    public String planId;
    public String changedToSubId;
    public PendingChange pendingChange;

    /**
     * Fold an authoritative /subscriptions/detail response into this row:
     * on-chain facts (state, charge watermark, schedule) always win, but a detail that
     * OMITS a field must never clobber a prior real value back to null/0 — notably planId
     * (empty string preserved-through), planTier (0 preserved-through), startAt and
     * billingAnchorAt (0 = "not yet backfilled"). Callers persist the row themselves.
     */
    public void applyDetail(QueryResp latest) {
        if (latest == null) {
            return;
        }
        state = latest.state;
        lastChargedPeriod = latest.lastChargedPeriod;
        periodMode = latest.periodMode;
        nextChargeableAt = latest.nextChargeableAt;
        changedToSubId = latest.changedToSubId;
        if (latest.payer != null && !latest.payer.isEmpty()) {
            payer = latest.payer;
        }
        if (latest.merchant != null) {
            merchant = latest.merchant;
        }
        if (latest.token != null) {
            token = latest.token;
        }
        if (latest.amountPerPeriod != null) {
            amountPerPeriod = latest.amountPerPeriod;
        }
        if (latest.startAt > 0) {
            startAt = latest.startAt;
        }
        if (latest.billingAnchorAt > 0) {
            billingAnchorAt = latest.billingAnchorAt;
        }
        // periodSec == 0 is the TRUE value for calendar-month subs; for fixed mode a 0 means
        // "omitted by the backend" and must not wipe the known cadence.
        if (latest.periodSec > 0 || PeriodMode.isCalendarMonth(latest.periodMode)) {
            periodSec = latest.periodSec;
        }
        if (latest.maxPeriods > 0) {
            maxPeriods = (int) latest.maxPeriods;
        }
        if (latest.planId != null && !latest.planId.isEmpty()) {
            planId = latest.planId;
        }
        if (latest.planTier != null && latest.planTier != 0) {
            planTier = latest.planTier;
        }
        updatedAt = System.currentTimeMillis() / 1000;
    }

    /** Field-for-field copy for the clone-on-get / clone-on-put discipline — see InMemorySubscriptionStore. */
    public StoredSubscription copy() {
        StoredSubscription c = new StoredSubscription();
        c.subId = subId;
        c.state = state;
        c.payer = payer;
        c.merchant = merchant;
        c.token = token;
        c.amountPerPeriod = amountPerPeriod;
        c.periodSec = periodSec;
        c.periodMode = periodMode;
        c.maxPeriods = maxPeriods;
        c.startAt = startAt;
        c.billingAnchorAt = billingAnchorAt;
        c.lastChargedPeriod = lastChargedPeriod;
        c.nextChargeableAt = nextChargeableAt;
        c.updatedAt = updatedAt;
        c.planTier = planTier;
        c.planId = planId;
        c.changedToSubId = changedToSubId;
        if (pendingChange != null) {
            c.pendingChange = new PendingChange();
            c.pendingChange.newSubId = pendingChange.newSubId;
            c.pendingChange.state = pendingChange.state;
            c.pendingChange.newPlanTier = pendingChange.newPlanTier;
        }
        return c;
    }

    public static class PendingChange {
        public String newSubId;
        public int state;
        public int newPlanTier;
    }
}
