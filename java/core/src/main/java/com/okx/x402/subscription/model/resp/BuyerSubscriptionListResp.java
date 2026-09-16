// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

/**
 * GET /buyers/{buyer}/subscriptions response. Merchant identity (merchant address / merchantId /
 * facilitator / subscriptionContract) is deliberately absent — mirrors the facilitator's
 * response shape.
 */
@JsonIgnoreProperties(ignoreUnknown = true)
public class BuyerSubscriptionListResp {
    public List<BuyerSubItem> subscriptions;

    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class BuyerSubItem {
        @JsonProperty("chainIndex")
        public long chainIndex;
        public String subId;
        public int state;
        public String payer;
        public String token;
        @JsonProperty("amountPerPeriod")
        public String amountPerPeriod;
        @JsonProperty("periodSec")
        public long periodSec;
        @JsonProperty("periodMode")
        public int periodMode;
        @JsonProperty("maxPeriods")
        public long maxPeriods;
        @JsonProperty("startAt")
        public long startAt;
        @JsonProperty("billingAnchorAt")
        public long billingAnchorAt;
        @JsonProperty("initialChargePeriods")
        public Long initialChargePeriods;
        @JsonProperty("initialChargeAmount")
        public String initialChargeAmount;
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
        @JsonProperty("currentPeriod")
        public long currentPeriod;
        /** Uncapped wall-clock period; &gt; maxPeriods ⇔ service window ended. */
        @JsonProperty("elapsedPeriods")
        public long elapsedPeriods;
        @JsonProperty("nextChargeableAt")
        public Long nextChargeableAt;
    }
}
