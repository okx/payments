// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.model.v2.PaymentRequirements;

import java.util.List;

/**
 * Client-side strategy for choosing one {@link PaymentRequirements} from the
 * server's 402 {@code accepts} list when multiple payment options are
 * offered (multiple schemes, multiple currencies, etc.). Mirrors Go's
 * {@code PaymentRequirementsSelector} and TypeScript's {@code SelectPaymentRequirements}.
 *
 * <p>Callers inject a custom selector via {@link OKXHttpClientConfig#paymentRequirementsSelector}
 * when they need non-default preferences (e.g. prefer USDG over USDT, or
 * prefer the {@code aggr_deferred} scheme). The default implementation
 * {@link #defaultSelector(String)} keeps the legacy behaviour: pick the
 * first option whose {@code network} equals the client's configured
 * network, otherwise fall back to the first option.
 */
@FunctionalInterface
public interface PaymentRequirementsSelector {

    /**
     * @param x402Version protocol version from the 402 envelope
     * @param accepts the full list of payment options from the server (never empty)
     * @return the chosen option — must be one of {@code accepts}
     */
    PaymentRequirements select(int x402Version, List<PaymentRequirements> accepts);

    /**
     * The default selector: network-match-first, else the first option.
     * Preserves the behaviour of prior SDK releases that hardcoded this
     * logic inside {@code OKXHttpClient}.
     */
    static PaymentRequirementsSelector defaultSelector(String preferredNetwork) {
        return (version, accepts) -> accepts.stream()
                .filter(r -> preferredNetwork != null && preferredNetwork.equals(r.network))
                .findFirst()
                .orElse(accepts.get(0));
    }
}
