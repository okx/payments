// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.model.v1.PaymentRequirements;
import com.okx.x402.util.Json;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Synchronous V1 facilitator client using Java 17 HttpClient.
 * Kept for backward compatibility with Coinbase CDP facilitator.
 */
public class HttpFacilitatorClient {

    private final HttpClient http =
            HttpClient.newBuilder()
                      .connectTimeout(Duration.ofSeconds(5))
                      .build();

    private final String baseUrl;

    /**
     * Creates a new HTTP facilitator client.
     *
     * @param baseUrl the base URL of the facilitator service
     */
    public HttpFacilitatorClient(String baseUrl) {
        this.baseUrl = baseUrl.endsWith("/")
                ? baseUrl.substring(0, baseUrl.length() - 1)
                : baseUrl;
    }

    /**
     * Verifies a payment header against the given requirements.
     *
     * @param paymentHeader the payment header to verify
     * @param req the payment requirements
     * @return verification response
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    public VerificationResponse verify(String paymentHeader,
                                       PaymentRequirements req)
            throws IOException, InterruptedException {

        Map<String, Object> body = Map.of(
                "x402Version", 1,
                "paymentHeader", paymentHeader,
                "paymentRequirements", req
        );

        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/verify"))
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(
                        Json.MAPPER.writeValueAsString(body)))
                .build();

        HttpResponse<String> response = http.send(request, HttpResponse.BodyHandlers.ofString());
        if (response.statusCode() != 200) {
            throw new IOException("HTTP " + response.statusCode() + ": " + response.body());
        }
        return Json.MAPPER.readValue(response.body(), VerificationResponse.class);
    }

    /**
     * Settles a verified payment.
     *
     * @param paymentHeader the payment header to settle
     * @param req the payment requirements
     * @return settlement response
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    public SettlementResponse settle(String paymentHeader,
                                     PaymentRequirements req)
            throws IOException, InterruptedException {

        Map<String, Object> body = Map.of(
                "x402Version", 1,
                "paymentHeader", paymentHeader,
                "paymentRequirements", req
        );

        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/settle"))
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(
                        Json.MAPPER.writeValueAsString(body)))
                .build();

        HttpResponse<String> response = http.send(request, HttpResponse.BodyHandlers.ofString());
        if (response.statusCode() != 200) {
            throw new IOException("HTTP " + response.statusCode() + ": " + response.body());
        }
        return Json.MAPPER.readValue(response.body(), SettlementResponse.class);
    }

    /**
     * Retrieves supported payment kinds.
     *
     * @return set of supported payment kinds
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    @SuppressWarnings("unchecked")
    public Set<Kind> supported() throws IOException, InterruptedException {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + "/supported"))
                .GET()
                .build();

        HttpResponse<String> response = http.send(request, HttpResponse.BodyHandlers.ofString());
        if (response.statusCode() != 200) {
            throw new IOException("HTTP " + response.statusCode() + ": " + response.body());
        }

        Map<String, Object> map = Json.MAPPER.readValue(response.body(), Map.class);
        List<?> kinds = (List<?>) map.getOrDefault("kinds", List.of());

        Set<Kind> out = new HashSet<>();
        for (Object k : kinds) {
            out.add(Json.MAPPER.convertValue(k, Kind.class));
        }
        return out;
    }
}
