// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

import com.fasterxml.jackson.annotation.JsonProperty;

public class SubscriptionTerms {
    public String payer;
    public String merchant;
    public String facilitator;
    public String token;
    @JsonProperty("amountPerPeriod")
    public String amountPerPeriod;
    @JsonProperty("periodSec")
    public long periodSec;
    @JsonProperty("maxPeriods")
    public int maxPeriods;
    @JsonProperty("startAt")
    public long startAt;
    @JsonProperty("initialChargePeriods")
    public int initialChargePeriods;
    @JsonProperty("initialChargeAmount")
    public String initialChargeAmount;
    @JsonProperty("termsDeadline")
    public long termsDeadline;
    @JsonProperty("permitHash")
    public String permitHash;
    public String salt;
    @JsonProperty("planTier")
    public int planTier;
    @JsonProperty("changeFromSubId")
    public String changeFromSubId;
    @JsonProperty("changeEffectiveAt")
    public int changeEffectiveAt;
    /**
     * PeriodMode: 0 = FIXED_SECONDS / 1 = CALENDAR_MONTH (17th EIP-712 field;
     * CALENDAR_MONTH requires periodSec == 0).
     */
    @JsonProperty("periodMode")
    public int periodMode;
    @JsonProperty("planId")
    public String planId;
}
