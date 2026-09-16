// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;

import java.io.IOException;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Routes facilitator calls to the appropriate client based on network.
 * Auto-routes X Layer networks to OKX, with configurable defaults.
 */
public class FacilitatorRouter implements FacilitatorClient {

    private final Map<String, FacilitatorClient> routeMap;
    private final FacilitatorClient defaultClient;

    private FacilitatorRouter(Builder builder) {
        this.routeMap = new HashMap<>(builder.routes);
        this.defaultClient = builder.defaultClient;
    }

    /** Returns a new builder for FacilitatorRouter. */
    public static Builder builder() {
        return new Builder();
    }

    @Override
    public VerifyResponse verify(PaymentPayload payload, PaymentRequirements req)
            throws IOException, InterruptedException {
        return getClient(req.network).verify(payload, req);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload, PaymentRequirements req)
            throws IOException, InterruptedException {
        return getClient(req.network).settle(payload, req);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload, PaymentRequirements req,
                                 boolean syncSettle)
            throws IOException, InterruptedException {
        return getClient(req.network).settle(payload, req, syncSettle);
    }

    @Override
    public SettleResponse settleStatus(String txHash)
            throws IOException, InterruptedException {
        if (defaultClient != null) {
            return defaultClient.settleStatus(txHash);
        }
        // HashMap.values() has no stable order, so iterating and returning the
        // first client would route to an unpredictable facilitator when the
        // router fronts multiple distinct clients. Allow it only when the
        // target is unambiguous (exactly one distinct client across all
        // networks); otherwise force the caller to declare a default.
        Set<FacilitatorClient> distinct = new HashSet<>(routeMap.values());
        if (distinct.size() == 1) {
            return distinct.iterator().next().settleStatus(txHash);
        }
        if (distinct.isEmpty()) {
            throw new IllegalStateException(
                    "No facilitator configured for settleStatus");
        }
        throw new IllegalStateException(
                "settleStatus is ambiguous across " + distinct.size()
                        + " distinct facilitator clients. Register a default"
                        + " via FacilitatorRouter.builder().defaultFacilitator(...)"
                        + " to resolve.");
    }

    @Override
    public SupportedResponse supported() throws IOException, InterruptedException {
        SupportedResponse merged = new SupportedResponse();
        Set<FacilitatorClient> visited = new HashSet<>();

        if (defaultClient != null && visited.add(defaultClient)) {
            mergeSupported(merged, defaultClient.supported());
        }
        for (FacilitatorClient client : routeMap.values()) {
            if (visited.add(client)) {
                mergeSupported(merged, client.supported());
            }
        }
        return merged;
    }

    private static void mergeSupported(SupportedResponse target, SupportedResponse source) {
        if (source.kinds != null) {
            target.kinds.addAll(source.kinds);
        }
        if (source.extensions != null) {
            for (String ext : source.extensions) {
                if (!target.extensions.contains(ext)) {
                    target.extensions.add(ext);
                }
            }
        }
        if (source.signers != null) {
            source.signers.forEach((key, addrs) ->
                    target.signers.merge(key, new ArrayList<>(addrs), (existing, incoming) -> {
                        for (String a : incoming) {
                            if (!existing.contains(a)) {
                                existing.add(a);
                            }
                        }
                        return existing;
                    }));
        }
    }

    private FacilitatorClient getClient(String network) {
        FacilitatorClient client = routeMap.get(network);
        if (client != null) {
            return client;
        }
        if (defaultClient != null) {
            return defaultClient;
        }
        throw new IllegalStateException("No facilitator configured for network: " + network);
    }

    /** Builder for FacilitatorRouter. */
    public static class Builder {
        private final Map<String, FacilitatorClient> routes = new HashMap<>();
        private FacilitatorClient defaultClient;

        /**
         * Configure OKX as the facilitator for X Layer networks.
         *
         * @param apiKey OKX API key
         * @param secretKey OKX secret key
         * @param passphrase OKX passphrase
         * @return this builder
         */
        public Builder okx(String apiKey, String secretKey, String passphrase) {
            OKXFacilitatorClient okxClient = new OKXFacilitatorClient(apiKey, secretKey, passphrase);
            routes.put("eip155:196", okxClient);
            routes.put("eip155:195", okxClient);
            return this;
        }

        /**
         * Add a custom route for a specific network.
         *
         * @param network CAIP-2 network identifier
         * @param client facilitator client for that network
         * @return this builder
         */
        public Builder route(String network, FacilitatorClient client) {
            routes.put(network, client);
            return this;
        }

        /**
         * Set the default facilitator for unmatched networks.
         *
         * @param client default facilitator client
         * @return this builder
         */
        public Builder defaultFacilitator(FacilitatorClient client) {
            this.defaultClient = client;
            return this;
        }

        /** Builds the FacilitatorRouter instance. */
        public FacilitatorRouter build() {
            return new FacilitatorRouter(this);
        }
    }
}
