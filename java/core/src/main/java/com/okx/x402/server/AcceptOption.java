// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import java.util.Map;

/**
 * One accepted payment option for a route — mirrors the {@code PaymentOption}
 * model used by the Go and TypeScript SDKs. A route may list multiple
 * {@code AcceptOption} instances (e.g. USDT + USDG, or {@code exact} +
 * {@code aggr_deferred}); each one becomes a separate {@code PaymentRequirements}
 * entry in the {@code accepts} array of the 402 {@code PAYMENT-REQUIRED}
 * envelope.
 *
 * <p>Fields are public-mutable so callers can populate them directly (matching
 * the style of {@link PaymentProcessor.RouteConfig}). A fluent
 * {@link #builder()} is provided for {@code List.of(...)} ergonomics.
 *
 * <p>Any field left null falls back to the owning {@link PaymentProcessor.RouteConfig}
 * at resolve time — so a route-level default (e.g. {@code network}, {@code payTo})
 * does not need to be repeated on every option.
 */
public class AcceptOption {

    /** Payment scheme identifier (e.g. {@code "exact"}, {@code "aggr_deferred"}). */
    public String scheme;

    /** CAIP-2 network identifier (e.g. {@code "eip155:196"}). */
    public String network;

    /** Recipient wallet address. */
    public String payTo;

    /**
     * USD price string (e.g. {@code "$0.01"}) or atomic-units string when
     * bypassing USD conversion. Exactly one of {@code price} /
     * {@code priceFunction} should be set.
     */
    public String price;

    /** Dynamic per-request price. Takes precedence over {@link #price} when non-null. */
    public PaymentProcessor.DynamicPrice priceFunction;

    /** Token contract address. When null, falls back to the registry default for {@link #network}. */
    public String asset;

    /** Maximum timeout in seconds for payment validity. 0 means "inherit from route". */
    public int maxTimeoutSeconds;

    /**
     * Extra scheme- or asset-specific fields merged into {@code PaymentRequirements.extra}.
     * When null, the asset registry's EIP-712 domain fields are used as-is.
     */
    public Map<String, Object> extra;

    public AcceptOption() {
    }

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private final AcceptOption opt = new AcceptOption();

        public Builder scheme(String s) { opt.scheme = s; return this; }
        public Builder network(String s) { opt.network = s; return this; }
        public Builder payTo(String s) { opt.payTo = s; return this; }
        public Builder price(String s) { opt.price = s; return this; }
        public Builder priceFunction(PaymentProcessor.DynamicPrice f) { opt.priceFunction = f; return this; }
        public Builder asset(String s) { opt.asset = s; return this; }
        public Builder maxTimeoutSeconds(int s) { opt.maxTimeoutSeconds = s; return this; }
        public Builder extra(Map<String, Object> e) { opt.extra = e; return this; }

        public AcceptOption build() {
            return opt;
        }
    }
}
