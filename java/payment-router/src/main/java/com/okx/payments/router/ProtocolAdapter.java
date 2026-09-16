package com.okx.payments.router;

import javax.servlet.FilterChain;
import javax.servlet.ServletException;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

/**
 * Cross-protocol payment adapter contract — D-UPM §3 / §10.10.
 *
 * <p>Hard rules ALL implementations must respect:
 * <ul>
 *   <li>{@link #detect} is purely synchronous and reads only headers (must NOT consume the body)</li>
 *   <li>{@link #getChallenge} is invoked concurrently with peers; failure surfaces via {@link PaymentRouterConfig#onError},
 *       never raises 500</li>
 *   <li>The returned challenge map's values are merged into multi-line same-name response headers
 *       (e.g. multiple {@code WWW-Authenticate}); the router uses {@code addHeader} per value</li>
 *   <li>SDK clients passed to the adapter constructor MUST be eagerly initialized (fail-fast)</li>
 * </ul>
 */
public interface ProtocolAdapter {

    /** Adapter name — must match the route-config key for this protocol (e.g. {@code "mpp"}, {@code "x402"}). */
    String name();

    /** Detect priority — lower runs first; default convention: {@code mpp=10}, {@code x402=20}. */
    int priority();

    /** Synchronous header-only check. Throwing here is treated as no-match (never 500). */
    boolean detect(HttpServletRequest request);

    /**
     * Compute the 402 challenge headers for this protocol given the per-route config slice.
     * Return {@code null} or empty to opt out of merging.
     */
    CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest request, Object routeAdapterConfig);

    /**
     * Handle a request that {@link #detect} matched. Three patterns are supported — the adapter decides:
     *
     * <ol>
     *   <li><b>Terminal</b> — write status/headers/body directly to {@code response} and do <em>not</em> call
     *       {@code chain.doFilter}. Used for 402 emission, charge settle, session-manage endpoints.</li>
     *   <li><b>Wrap (pre + chain + post)</b> — verify, then call {@code chain.doFilter(req, resp)} so the
     *       merchant's downstream handler runs, then inject post-processing headers (e.g.
     *       {@code Payment-Receipt}, {@code X-Spent}) on success. Mirrors {@code com.okx.x402.server.PaymentFilter}
     *       and Rust {@code MppVerifyService::call}.</li>
     *   <li><b>Pass-through</b> — call {@code chain.doFilter} without modification (rare; usually the router
     *       short-circuits before reaching {@code handle}).</li>
     * </ol>
     *
     * <p>Implementations are responsible for not double-writing the response (check {@link HttpServletResponse#isCommitted})
     * and for surfacing exceptions in {@link PaymentRouterConfig#onError} as appropriate.
     */
    void handle(HttpServletRequest request, HttpServletResponse response, FilterChain chain,
                Object routeAdapterConfig) throws IOException, ServletException;
}
