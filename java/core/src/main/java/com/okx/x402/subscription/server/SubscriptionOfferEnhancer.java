// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.model.v2.SupportedKind;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.server.AcceptOption;
import com.okx.x402.server.PaymentProcessor;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Injects the subscription three-some — {@code contracts {subscription, permit2}},
 * {@code facilitator} and the EIP-712 {@code domain} — into subscription AcceptOptions from the
 * facilitator's {@code /supported} broadcast. Caller-set values always win; the facilitator EOA is
 * read from {@code facilitatorAddress} (the key other schemes use) with a fallback to
 * {@code facilitator} (the period scheme's legacy key).
 */
public final class SubscriptionOfferEnhancer {

    private SubscriptionOfferEnhancer() {}

    /**
     * Enhance every subscription-scheme option in place. Options whose network has no matching
     * subscription kind in {@code supported} are left untouched (the seller may have pinned
     * everything by hand).
     */
    public static void enhance(List<AcceptOption> accepts, SupportedResponse supported) {
        if (accepts == null || supported == null || supported.kinds == null) {
            return;
        }
        for (AcceptOption option : accepts) {
            if (!PaymentProcessor.isSubscriptionSchemeName(option.scheme)) {
                continue;
            }
            SupportedKind kind = findKind(supported, option.network);
            if (kind == null || kind.extra == null) {
                continue;
            }
            Map<String, Object> extra = option.extra != null
                    ? option.extra : new LinkedHashMap<>();
            option.extra = extra;

            String facilitator = str(kind.extra.get("facilitatorAddress"));
            if (facilitator == null) {
                facilitator = str(kind.extra.get("facilitator"));
            }
            if (!extra.containsKey("facilitator") && facilitator != null) {
                extra.put("facilitator", facilitator);
            }

            String subscription = str(kind.extra.get("subscriptionContract"));
            String permit2 = str(kind.extra.get("permit2Contract"));
            if (!extra.containsKey("contracts") && subscription != null && permit2 != null) {
                extra.put("contracts", Map.of(
                        "subscription", subscription, "permit2", permit2));
            }

            if (!extra.containsKey("domain") && subscription != null) {
                Long chainId = chainIdOf(option.network);
                if (chainId != null) {
                    extra.put("domain", Map.of(
                            "name", "A2APaySubscription",
                            "version", "1",
                            "chainId", chainId,
                            "verifyingContract", subscription));
                }
            }
        }
    }

    private static SupportedKind findKind(SupportedResponse supported, String network) {
        for (SupportedKind kind : supported.kinds) {
            if (PaymentProcessor.isSubscriptionSchemeName(kind.scheme)
                    && kind.network != null && kind.network.equalsIgnoreCase(network)) {
                return kind;
            }
        }
        return null;
    }

    private static Long chainIdOf(String network) {
        if (network == null || !network.startsWith("eip155:")) {
            return null;
        }
        try {
            return Long.parseLong(network.substring("eip155:".length()));
        } catch (NumberFormatException e) {
            return null;
        }
    }

    private static String str(Object v) {
        return v instanceof String s && !s.isEmpty() ? s : null;
    }
}
