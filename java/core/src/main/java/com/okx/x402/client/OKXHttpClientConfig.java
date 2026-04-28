// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.crypto.EvmSigner;

import java.net.http.HttpClient;
import java.time.Duration;
import java.util.Objects;

/**
 * Configuration for {@link OKXHttpClient}. Mirrors the TypeScript SDK's
 * options-object shape: the required signer is a final constructor
 * argument, optional fields are public and mutable with sensible
 * defaults.
 *
 * <p>Example:
 * <pre>
 * OKXHttpClientConfig cfg = new OKXHttpClientConfig(signer);
 * cfg.network = "eip155:195";
 * cfg.requestTimeout = Duration.ofSeconds(60);
 * cfg.httpClient = mySharedHttpClient;   // optional injection
 * OKXHttpClient client = new OKXHttpClient(cfg);
 * </pre>
 *
 * <p>When {@link #httpClient} is non-null, {@link #connectTimeout} is
 * ignored. {@link #requestTimeout} is applied to the built request on
 * the convenience {@link OKXHttpClient#get(java.net.URI)} path.
 * Callers of the lower-level {@code request(HttpRequest)} API control
 * their own request timeouts via {@code HttpRequest.Builder.timeout()}.
 */
public class OKXHttpClientConfig {

    // Required
    public final EvmSigner signer;

    // Optional — tweak after construction
    public String network = "eip155:196";
    public Duration connectTimeout = Duration.ofSeconds(10);
    public Duration requestTimeout = Duration.ofSeconds(30);
    public HttpClient httpClient;

    /**
     * Strategy for picking one {@link com.okx.x402.model.v2.PaymentRequirements}
     * out of the server's 402 {@code accepts} list. When null, a
     * default selector is used: match by {@link #network}, else first.
     * Inject a custom selector to prefer a specific asset address,
     * scheme, or EIP-712 domain name.
     */
    public PaymentRequirementsSelector paymentRequirementsSelector;

    public OKXHttpClientConfig(EvmSigner signer) {
        this.signer = Objects.requireNonNull(signer, "signer");
    }
}
