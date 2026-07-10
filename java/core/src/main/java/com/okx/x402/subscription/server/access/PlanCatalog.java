// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.access;

import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

public class PlanCatalog {

    private final Map<String, PlanEntry> plans = new ConcurrentHashMap<>();

    /**
     * Register a plan. Tiers MUST be unique across the catalog: the
     * upgrade/downgrade direction of a plan change is decided purely by tier comparison, so two
     * plans sharing a tier would make the direction ambiguous (runtime falls back to the
     * downgrade path, and the contract rejects true same-tier changes with tier_same).
     *
     * @throws IllegalArgumentException when another registered plan already uses the same tier
     */
    public PlanCatalog register(String planId, PlanEntry entry) {
        for (Map.Entry<String, PlanEntry> existing : plans.entrySet()) {
            if (!existing.getKey().equals(planId) && existing.getValue().tier == entry.tier) {
                throw new IllegalArgumentException("duplicate plan tier " + entry.tier
                        + ": '" + planId + "' collides with '" + existing.getKey()
                        + "' — plan tiers must be unique");
            }
        }
        plans.put(planId, entry);
        return this;
    }

    public PlanEntry get(String planId) {
        return plans.get(planId);
    }

    public PlanEntry getByTier(int tier) {
        return plans.values().stream()
                .filter(p -> p.tier == tier)
                .findFirst().orElse(null);
    }

    /** All registered plans, keyed by planId (read-only view). */
    public Map<String, PlanEntry> all() {
        return Collections.unmodifiableMap(plans);
    }

    public Collection<String> planIds() {
        return Collections.unmodifiableSet(plans.keySet());
    }

    public static class PlanEntry {
        public final int tier;
        public final String amountPerPeriod;
        public final long periodSec;
        /** 0 = FIXED_SECONDS / 1 = CALENDAR_MONTH (periodSec must be 0 for calendar mode). */
        public final int periodMode;
        public final int maxPeriods;
        /** Periods pre-charged at subscribe (0 = no separate initial charge). */
        public final int initialChargePeriods;
        public final String initialChargeAmount;
        /**
         * First-charge price string for AcceptOption.price (e.g. "$0.000001"), resolved through
         * AssetRegistry when the option rides a processor route. Null → falls back to the atomic
         * initialChargeAmount (standalone /changePlan responses only).
         */
        public final String price;
        public final List<String> features;
        public final String asset;
        public final String payTo;

        public PlanEntry(int tier, String amountPerPeriod, long periodSec, int maxPeriods,
                         String initialChargeAmount, List<String> features, String asset, String payTo) {
            this(tier, amountPerPeriod, periodSec, 0, maxPeriods,
                    hasAmount(initialChargeAmount) ? 1 : 0, initialChargeAmount, null,
                    features, asset, payTo);
        }

        public PlanEntry(int tier, String amountPerPeriod, long periodSec, int periodMode,
                         int maxPeriods, String initialChargeAmount, List<String> features,
                         String asset, String payTo) {
            this(tier, amountPerPeriod, periodSec, periodMode, maxPeriods,
                    hasAmount(initialChargeAmount) ? 1 : 0, initialChargeAmount, null,
                    features, asset, payTo);
        }

        public PlanEntry(int tier, String amountPerPeriod, long periodSec, int periodMode,
                         int maxPeriods, int initialChargePeriods, String initialChargeAmount,
                         String price, List<String> features, String asset, String payTo) {
            this.tier = tier;
            this.amountPerPeriod = amountPerPeriod;
            this.periodSec = periodSec;
            this.periodMode = periodMode;
            this.maxPeriods = maxPeriods;
            this.initialChargePeriods = initialChargePeriods;
            this.initialChargeAmount = initialChargeAmount;
            this.price = price;
            this.features = features != null ? List.copyOf(features) : List.of();
            this.asset = asset;
            this.payTo = payTo;
        }

        private static boolean hasAmount(String amount) {
            return amount != null && !amount.isEmpty() && !"0".equals(amount);
        }
    }
}
