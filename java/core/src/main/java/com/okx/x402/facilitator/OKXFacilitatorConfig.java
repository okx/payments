// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import java.net.http.HttpClient;
import java.time.Duration;

/**
 * Configuration for {@link OKXFacilitatorClient}. Mirrors the TypeScript
 * {@code OKXConfig} options-object shape: required credentials are final
 * constructor arguments, optional fields are public and mutable with
 * sensible defaults.
 *
 * <p>Example:
 * <pre>
 * OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
 * cfg.baseUrl = System.getenv("OKX_FACILITATOR_BASE_URL");
 * cfg.requestTimeout = Duration.ofSeconds(60);
 * cfg.httpClient = mySharedHttpClient;   // optional injection
 * OKXFacilitatorClient client = new OKXFacilitatorClient(cfg);
 * </pre>
 *
 * <p>When {@link #httpClient} is non-null, {@link #connectTimeout} is
 * ignored — the caller's client already carries its own connect
 * behaviour. {@link #requestTimeout} is still honoured because it is
 * applied per-request via {@code HttpRequest.Builder.timeout()}.
 */
public class OKXFacilitatorConfig {

    // Required
    public final String apiKey;
    public final String secretKey;
    public final String passphrase;

    // Optional — tweak after construction
    public String baseUrl = "https://www.okx.com";
    public Duration connectTimeout = Duration.ofSeconds(10);
    public Duration requestTimeout = Duration.ofSeconds(30);

    /**
     * Caller-supplied JDK {@link HttpClient}. Convenience field for teams
     * that want to share a JDK-backed client (connection pool, custom
     * {@code Executor}, {@code SSLContext}, proxy, {@code Authenticator})
     * without writing their own {@link HttpExecutor}. Ignored when
     * {@link #httpExecutor} is also set. When this is supplied,
     * {@link #connectTimeout} is ignored — the caller's client carries
     * its own connect behaviour.
     */
    public HttpClient httpClient;

    /**
     * Caller-supplied HTTP executor — the full escape hatch. Takes
     * precedence over {@link #httpClient}. Implement this to route
     * facilitator HTTP through OkHttp, Apache HttpClient, Reactor Netty,
     * or any other stack. See {@link HttpExecutor} Javadoc and
     * {@code CONFIG.md} for an OkHttp recipe.
     */
    public HttpExecutor httpExecutor;

    public OKXFacilitatorConfig(String apiKey, String secretKey, String passphrase) {
        // Credentials are not null-checked here; OKXAuth validates them
        // (rejecting null or empty with IllegalArgumentException) when the
        // OKXFacilitatorClient is constructed from this config.
        this.apiKey = apiKey;
        this.secretKey = secretKey;
        this.passphrase = passphrase;
    }
}
