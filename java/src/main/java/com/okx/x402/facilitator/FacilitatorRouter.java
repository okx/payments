package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;

import java.io.IOException;
import java.util.HashMap;
import java.util.Map;

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
        for (FacilitatorClient client : routeMap.values()) {
            return client.settleStatus(txHash);
        }
        throw new IllegalStateException("No facilitator configured for settleStatus");
    }

    @Override
    public SupportedResponse supported() throws IOException, InterruptedException {
        // Return from default client if available
        if (defaultClient != null) {
            return defaultClient.supported();
        }
        // Return first available
        for (FacilitatorClient client : routeMap.values()) {
            return client.supported();
        }
        return new SupportedResponse();
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
