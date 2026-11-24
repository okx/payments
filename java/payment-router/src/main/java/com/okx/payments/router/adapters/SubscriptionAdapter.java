// SPDX-License-Identifier: Apache-2.0
package com.okx.payments.router.adapters;

import com.okx.payments.router.ProtocolAdapter;

import javax.servlet.FilterChain;
import javax.servlet.ServletException;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

/**
 * Subscription {@link ProtocolAdapter} — detects the canonical {@code APP-Access} access
 * proof (plus the legacy {@code X-APP-Access} / {@code X-APP-PAYMENT} aliases) for the
 * {@code period} subscription scheme routing. Subscription PAYMENTS on the canonical
 * {@code PAYMENT-SIGNATURE} header are detected by {@link X402Adapter} and reach the same
 * {@code PaymentProcessor}; this adapter's job is the access-proof path.
 *
 * <p>Priority 5 — runs before x402 (20) and mpp (10) so that subscription access
 * proofs bypass payment detection entirely.
 */
public class SubscriptionAdapter implements ProtocolAdapter {

    static final String HEADER_APP_PAYMENT = "X-APP-PAYMENT";
    /** Canonical access-proof header (no X- prefix). */
    static final String HEADER_ACCESS = "APP-Access";
    /** Legacy alias (earlier releases); still detected inbound. */
    static final String HEADER_APP_ACCESS = "X-APP-Access";
    static final String HEADER_APP_PAYMENT_REQUIRED = "X-APP-PAYMENT-REQUIRED";

    public interface SubscriptionChallengeProvider {
        Map<String, List<String>> buildChallenge(HttpServletRequest request, Object routeConfig);
    }

    public interface SubscriptionRequestHandler {
        void handle(HttpServletRequest request, HttpServletResponse response,
                    FilterChain chain, Object routeConfig) throws IOException, ServletException;
    }

    private final SubscriptionChallengeProvider challengeProvider;
    private final SubscriptionRequestHandler requestHandler;

    public SubscriptionAdapter(SubscriptionChallengeProvider challengeProvider,
                               SubscriptionRequestHandler requestHandler) {
        this.challengeProvider = challengeProvider;
        this.requestHandler = requestHandler;
    }

    @Override
    public String name() {
        return "subscription";
    }

    @Override
    public int priority() {
        return 5;
    }

    @Override
    public boolean detect(HttpServletRequest request) {
        // Canonical APP-Access first (a subscriber sending only the canonical header must not
        // fall through to the generic 402 challenge), then the legacy aliases.
        String access = request.getHeader(HEADER_ACCESS);
        if (access != null && !access.isEmpty()) {
            return true;
        }
        String appPayment = request.getHeader(HEADER_APP_PAYMENT);
        if (appPayment != null && !appPayment.isEmpty()) {
            return true;
        }
        String appAccess = request.getHeader(HEADER_APP_ACCESS);
        return appAccess != null && !appAccess.isEmpty();
    }

    @Override
    public CompletableFuture<Map<String, List<String>>> getChallenge(
            HttpServletRequest request, Object routeAdapterConfig) {
        if (challengeProvider == null) {
            return CompletableFuture.completedFuture(null);
        }
        return CompletableFuture.supplyAsync(() ->
                challengeProvider.buildChallenge(request, routeAdapterConfig));
    }

    @Override
    public void handle(HttpServletRequest request, HttpServletResponse response,
                       FilterChain chain, Object routeAdapterConfig)
            throws IOException, ServletException {
        if (requestHandler != null) {
            requestHandler.handle(request, response, chain, routeAdapterConfig);
        } else {
            chain.doFilter(request, response);
        }
    }
}
