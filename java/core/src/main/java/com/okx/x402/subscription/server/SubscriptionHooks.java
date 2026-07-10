// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.subscription.server.store.StoredSubscription;

public final class SubscriptionHooks {

    private SubscriptionHooks() {}

    @FunctionalInterface
    public interface BeforeSubscribeHook {
        void beforeSubscribe(StoredSubscription sub);
    }

    @FunctionalInterface
    public interface AfterSubscribeHook {
        void afterSubscribe(StoredSubscription sub);
    }

    @FunctionalInterface
    public interface BeforeChargeHook {
        void beforeCharge(String subId);
    }

    @FunctionalInterface
    public interface AfterChargeHook {
        void afterCharge(String subId, int period);
    }

    @FunctionalInterface
    public interface OnChargeFailureHook {
        void onChargeFailure(String subId, String reason);
    }

    @FunctionalInterface
    public interface BeforeChangeHook {
        void beforeChange(String oldSubId, String newSubId);
    }

    @FunctionalInterface
    public interface AfterChangeHook {
        void afterChange(String oldSubId, String newSubId);
    }

    @FunctionalInterface
    public interface BeforeCancelHook {
        void beforeCancel(String subId);
    }

    @FunctionalInterface
    public interface AfterCancelHook {
        void afterCancel(String subId);
    }

    /**
     * Merchant-owned access override, invoked AFTER the AccessProof signature verifies and BEFORE
     * the period judgment. Deny short-circuits (the period judgment is skipped). Whether a buyer
     * of a merchant-canceled subscription may keep accessing until period end is the merchant's
     * call — implement it here.
     */
    @FunctionalInterface
    public interface OnBeforeAccessHook {
        AccessDecision beforeAccess(com.okx.x402.subscription.model.AccessProof proof,
                                    StoredSubscription sub);
    }

    /** Result of {@link OnBeforeAccessHook}. */
    public static final class AccessDecision {
        public final boolean denied;
        public final String reason;

        private AccessDecision(boolean denied, String reason) {
            this.denied = denied;
            this.reason = reason;
        }

        /** Continue to the period judgment. */
        public static AccessDecision proceed() {
            return new AccessDecision(false, null);
        }

        /** Deny the request; the period judgment is skipped. */
        public static AccessDecision deny(String reason) {
            return new AccessDecision(true, reason);
        }
    }
}
