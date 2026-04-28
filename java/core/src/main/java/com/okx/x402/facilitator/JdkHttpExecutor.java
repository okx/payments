// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Map;
import java.util.Objects;

/**
 * Default {@link HttpExecutor} backed by the JDK built-in
 * {@link java.net.http.HttpClient}. Used when an
 * {@link OKXFacilitatorConfig} supplies neither {@code httpExecutor}
 * nor a custom {@code httpClient}.
 *
 * <p>Supports {@code GET} and {@code POST}. Per-call timeout is applied
 * via {@link HttpRequest.Builder#timeout(Duration)}.
 */
public final class JdkHttpExecutor implements HttpExecutor {

    private final HttpClient http;

    public JdkHttpExecutor(HttpClient http) {
        this.http = Objects.requireNonNull(http, "http");
    }

    @Override
    public HttpExecResult execute(String method, URI uri, String body,
                                  Map<String, String> headers, Duration timeout)
            throws IOException, InterruptedException {
        HttpRequest.Builder rb = HttpRequest.newBuilder()
                .uri(uri)
                .timeout(timeout);
        if ("POST".equals(method)) {
            rb.POST(HttpRequest.BodyPublishers.ofString(body == null ? "" : body));
        } else if ("GET".equals(method)) {
            rb.GET();
        } else {
            throw new IOException("Unsupported method: " + method);
        }
        headers.forEach(rb::header);

        HttpResponse<String> resp = http.send(rb.build(),
                HttpResponse.BodyHandlers.ofString());
        return new HttpExecResult(resp.statusCode(), resp.body());
    }
}
