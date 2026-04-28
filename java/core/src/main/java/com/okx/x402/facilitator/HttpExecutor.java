// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import java.io.IOException;
import java.net.URI;
import java.time.Duration;
import java.util.Map;

/**
 * Pluggable HTTP execution SPI used by {@link OKXFacilitatorClient}.
 *
 * <p>Implement this interface to route the facilitator's HTTP calls
 * through any HTTP stack (OkHttp, Apache HttpClient, Reactor Netty,
 * etc.). All OKX-specific protocol logic — HMAC auth header
 * generation, envelope unwrapping, error-code mapping, retry on
 * 429 / {@code 50011} — stays inside the facilitator client; an
 * executor is responsible only for raw HTTP execution.
 *
 * <p>Default implementation: {@link JdkHttpExecutor}.
 *
 * <p>Example OkHttp adapter (~25 lines; see {@code CONFIG.md} for the
 * full recipe):
 * <pre>
 * public class OkHttpExecutor implements HttpExecutor {
 *     private static final MediaType JSON = MediaType.get("application/json");
 *     private final OkHttpClient http;
 *     public OkHttpExecutor(OkHttpClient http) { this.http = http; }
 *     public HttpExecResult execute(String method, URI uri, String body,
 *             Map&lt;String,String&gt; headers, Duration timeout) throws IOException {
 *         OkHttpClient perCall = http.newBuilder().callTimeout(timeout).build();
 *         Request.Builder rb = new Request.Builder().url(uri.toString());
 *         headers.forEach(rb::header);
 *         if ("POST".equals(method)) rb.post(RequestBody.create(body, JSON));
 *         else rb.get();
 *         try (Response r = perCall.newCall(rb.build()).execute()) {
 *             return new HttpExecResult(r.code(), r.body() != null ? r.body().string() : "");
 *         }
 *     }
 * }
 * </pre>
 */
@FunctionalInterface
public interface HttpExecutor {

    /**
     * Execute a single HTTP call.
     *
     * @param method  HTTP method, currently {@code "GET"} or {@code "POST"}
     * @param uri     absolute request URI
     * @param body    request body (null or empty for GET)
     * @param headers request headers to apply (auth headers included)
     * @param timeout per-call timeout — the implementation must enforce it
     * @return the full response (status code + raw body)
     * @throws IOException on network error, timeout, or body-read failure
     * @throws InterruptedException if the call is interrupted
     */
    HttpExecResult execute(String method, URI uri, String body,
                           Map<String, String> headers, Duration timeout)
            throws IOException, InterruptedException;

    /**
     * Result of a single HTTP call: status code + raw body as UTF-8 string.
     * The facilitator client interprets status codes and OKX envelopes;
     * the executor does not.
     */
    record HttpExecResult(int statusCode, String body) {}
}
