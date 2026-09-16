// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model;

import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public class SubscriptionOffer {
    @JsonProperty("chainIndex")
    public int chainIndex;
    public Map<String, String> contracts;
    public String facilitator;
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
    public PlanInfo plan;
    public ChangeFromInfo changeFrom;
    public String allowanceStatusUrl;

    public static class PlanInfo {
        public String id;
        public int tier;
        public String name;
        public List<String> features;
    }

    public static class ChangeFromInfo {
        public String fromSubId;
        public int fromPlanTier;
        public String direction;
        public String effectiveAt;
    }

    @SuppressWarnings("unchecked")
    public static SubscriptionOffer fromExtra(Map<String, Object> extra) {
        if (extra == null) return null;
        SubscriptionOffer offer = new SubscriptionOffer();
        offer.chainIndex = extra.containsKey("chainIndex") ? ((Number) extra.get("chainIndex")).intValue() : 0;
        offer.contracts = (Map<String, String>) extra.get("contracts");
        offer.facilitator = (String) extra.get("facilitator");
        offer.amountPerPeriod = (String) extra.get("amountPerPeriod");
        offer.periodSec = extra.containsKey("periodSec") ? ((Number) extra.get("periodSec")).longValue() : 0;
        offer.maxPeriods = extra.containsKey("maxPeriods") ? ((Number) extra.get("maxPeriods")).intValue() : 0;
        offer.startAt = extra.containsKey("startAt") ? ((Number) extra.get("startAt")).longValue() : 0;
        offer.initialChargePeriods = extra.containsKey("initialChargePeriods") ? ((Number) extra.get("initialChargePeriods")).intValue() : 0;
        offer.initialChargeAmount = (String) extra.get("initialChargeAmount");
        // plan and changeFrom left as nested maps - callers parse if needed
        offer.allowanceStatusUrl = (String) extra.get("allowanceStatusUrl");
        return offer;
    }

    public Map<String, Object> toExtra() {
        Map<String, Object> m = new LinkedHashMap<>();
        m.put("chainIndex", chainIndex);
        if (contracts != null) m.put("contracts", contracts);
        if (facilitator != null) m.put("facilitator", facilitator);
        if (amountPerPeriod != null) m.put("amountPerPeriod", amountPerPeriod);
        m.put("periodSec", periodSec);
        m.put("maxPeriods", maxPeriods);
        m.put("startAt", startAt);
        m.put("initialChargePeriods", initialChargePeriods);
        if (initialChargeAmount != null) m.put("initialChargeAmount", initialChargeAmount);
        if (plan != null) {
            Map<String, Object> pm = new LinkedHashMap<>();
            pm.put("id", plan.id);
            pm.put("tier", plan.tier);
            pm.put("name", plan.name);
            if (plan.features != null) pm.put("features", plan.features);
            m.put("plan", pm);
        }
        if (changeFrom != null) {
            Map<String, Object> cf = new LinkedHashMap<>();
            cf.put("fromSubId", changeFrom.fromSubId);
            cf.put("fromPlanTier", changeFrom.fromPlanTier);
            cf.put("direction", changeFrom.direction);
            cf.put("effectiveAt", changeFrom.effectiveAt);
            m.put("changeFrom", cf);
        }
        if (allowanceStatusUrl != null) m.put("allowanceStatusUrl", allowanceStatusUrl);
        return m;
    }
}
