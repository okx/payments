// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * GET /api/v6/pay/x402/subscriptions/detail?subId= response (public read, no AK auth).
 * Mirrors the facilitator's response shape.
 */
@JsonIgnoreProperties(ignoreUnknown = true)
public class QueryResp {
    public String subId;
    public int state;
    public String payer;
    public String merchant;
    public String token;
    @JsonProperty("amountPerPeriod")
    public String amountPerPeriod;
    @JsonProperty("periodSec")
    public long periodSec;
    /** 0 = FIXED_SECONDS / 1 = CALENDAR_MONTH. */
    @JsonProperty("periodMode")
    public int periodMode;
    @JsonProperty("maxPeriods")
    public long maxPeriods;
    @JsonProperty("startAt")
    public long startAt;
    /** Calendar-month billing anchor (unix sec); 0 = not yet backfilled; ignored for fixed mode. */
    @JsonProperty("billingAnchorAt")
    public long billingAnchorAt;
    /** Highest period actually charged (on-chain fact, mirrors DB). */
    @JsonProperty("lastChargedPeriod")
    public long lastChargedPeriod;
    @JsonProperty("totalPulled")
    public String totalPulled;
    @JsonProperty("planId")
    public String planId;
    @JsonProperty("planTier")
    public Integer planTier;
    @JsonProperty("changedToSubId")
    public String changedToSubId;

    public boolean isActive;
    public boolean serviceEnded;
    /** Wall-clock period clamped to maxPeriods (mirrors contract; drives charging). */
    @JsonProperty("currentPeriod")
    public long currentPeriod;
    /**
     * Raw wall-clock period WITHOUT the maxPeriods cap. Keeps growing past maxPeriods so callers
     * can tell an expired subscription apart; access checks compare it against lastChargedPeriod.
     */
    @JsonProperty("elapsedPeriods")
    public long elapsedPeriods;
    /** Next chargeable boundary (unix sec); null when all periods are consumed. */
    @JsonProperty("nextChargeableAt")
    public Long nextChargeableAt;
    @JsonProperty("pendingPlanChange")
    public PendingPlanChange pendingPlanChange;

    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class PendingPlanChange {
        public String subId;
        public String newSubId;
        @JsonProperty("effectiveFromPeriod")
        public Long effectiveFromPeriod;
        public int state;
    }
}
