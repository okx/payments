// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.crypto.CryptoSignException;
import com.okx.x402.crypto.EvmSigner;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.util.Json;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Base64;
import java.util.Map;
import java.util.Objects;

/**
 * HTTP client with automatic x402 V2 payment handling.
 * Intercepts 402 responses, signs payment, and retries with PAYMENT-SIGNATURE header.
 */
public class OKXHttpClient {

    private final HttpClient http;
    private final EvmSigner signer;
    private final String network;
    private final Duration requestTimeout;
    private final PaymentRequirementsSelector selector;

    /**
     * Creates an OKXHttpClient with default X Layer mainnet network and
     * default timeouts (10s connect, 30s request).
     *
     * @param signer EVM signer for payment authorization
     */
    public OKXHttpClient(EvmSigner signer) {
        this(new OKXHttpClientConfig(signer));
    }

    /**
     * Creates an OKXHttpClient with the specified network and default
     * timeouts.
     *
     * @param signer EVM signer for payment authorization
     * @param network CAIP-2 network identifier
     */
    public OKXHttpClient(EvmSigner signer, String network) {
        this(initConfig(signer, network));
    }

    /**
     * Creates an OKXHttpClient from a full configuration object, including
     * optional timeout overrides and an optional caller-supplied
     * {@link HttpClient} (for connection pooling, proxy, tracing, etc.).
     */
    public OKXHttpClient(OKXHttpClientConfig config) {
        Objects.requireNonNull(config, "config");
        this.signer = config.signer;
        this.network = config.network;
        this.requestTimeout = Objects.requireNonNull(
                config.requestTimeout, "requestTimeout");
        this.http = config.httpClient != null
                ? config.httpClient
                : HttpClient.newBuilder()
                        .connectTimeout(Objects.requireNonNull(
                                config.connectTimeout, "connectTimeout"))
                        .build();
        this.selector = config.paymentRequirementsSelector != null
                ? config.paymentRequirementsSelector
                : PaymentRequirementsSelector.defaultSelector(config.network);
    }

    private static OKXHttpClientConfig initConfig(EvmSigner signer, String network) {
        OKXHttpClientConfig cfg = new OKXHttpClientConfig(signer);
        cfg.network = network;
        return cfg;
    }

    /**
     * GET with automatic 402 payment handling.
     *
     * @param uri target URI
     * @return HTTP response (200 after payment, or original non-402 response)
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    public HttpResponse<String> get(URI uri) throws IOException, InterruptedException {
        HttpRequest initial = HttpRequest.newBuilder()
                .uri(uri)
                .GET()
                .timeout(requestTimeout)
                .build();
        return request(initial);
    }

    /**
     * Generic request with auto 402 handling.
     * Preserves the original HTTP method, headers, and body on retry.
     *
     * <p>Behaviour on the retried request:
     * <ul>
     *   <li>If the retry returns any non-402 status (200, 4xx, 5xx, …) the
     *       response is returned to the caller as-is.</li>
     *   <li>If the retry <em>also</em> returns 402 (e.g. nonce already used,
     *       authorization expired, server-side replay protection), the
     *       second 402 response is returned to the caller — this method does
     *       NOT throw. Inspect {@link HttpResponse#statusCode()} and the
     *       {@code PAYMENT-REQUIRED} header / body to decide whether to
     *       swap signer / re-select requirements / surface the failure to
     *       end users.</li>
     * </ul>
     * Mirrors the TypeScript SDK's {@code x402Fetch} behaviour, where the
     * second 402 is returned rather than thrown so callers can implement
     * custom recovery (e.g. a {@code PaymentRequiredHook}).
     *
     * @param initial the initial HTTP request
     * @return HTTP response after handling any 402; the caller MUST inspect
     *         {@link HttpResponse#statusCode()} since 402 is a possible
     *         terminal status when the server rejects the signed payment.
     * @throws IOException if signing fails or the 402 envelope cannot be
     *         decoded (i.e. the server is not speaking x402); a 402 status
     *         on the retry response itself is not an error.
     * @throws InterruptedException if the request is interrupted
     */
    public HttpResponse<String> request(HttpRequest initial)
            throws IOException, InterruptedException {
        HttpResponse<String> resp = http.send(initial, HttpResponse.BodyHandlers.ofString());

        if (resp.statusCode() != 402) {
            return resp;
        }

        PaymentRequired paymentRequired = parsePaymentRequired(resp);

        // Select matching requirement
        PaymentRequirements selected = selectRequirement(
                paymentRequired.x402Version, paymentRequired.accepts);

        // Sign
        Map<String, Object> signedPayload;
        try {
            signedPayload = signer.signPaymentRequirements(selected);
        } catch (CryptoSignException e) {
            throw new IOException("Payment signing failed", e);
        }

        // Build V2 PaymentPayload
        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.accepted = selected;
        pp.resource = paymentRequired.resource;
        pp.payload = signedPayload;

        // Retry: copy original request (method, headers, body) and add payment header.
        // Whatever the retry returns — 200, 402, 4xx, 5xx — is returned verbatim;
        // we do NOT translate a second 402 into an exception, so callers can
        // implement custom recovery (re-sign with a different nonce, swap signer,
        // change selector, …) on top of statusCode() == 402.
        String header = pp.toHeader();
        HttpRequest retry = HttpRequest.newBuilder(initial, (name, value) -> true)
                .header("PAYMENT-SIGNATURE", header)
                .build();
        return http.send(retry, HttpResponse.BodyHandlers.ofString());
    }


    private static PaymentRequired parsePaymentRequired(HttpResponse<String> resp)
            throws IOException {
        String headerB64 = resp.headers()
                .firstValue("PAYMENT-REQUIRED")
                .orElse(null);
        if (headerB64 != null && !headerB64.isEmpty()) {
            byte[] decoded = Base64.getDecoder().decode(headerB64);
            return Json.MAPPER.readValue(decoded, PaymentRequired.class);
        }
        String body = resp.body();
        if (body == null || body.isEmpty() || "{}".equals(body.trim())) {
            throw new IOException(
                    "402 response missing PAYMENT-REQUIRED header and body is empty");
        }
        return Json.MAPPER.readValue(body, PaymentRequired.class);
    }


    private PaymentRequirements selectRequirement(
            int x402Version, java.util.List<PaymentRequirements> accepts) {
        if (accepts == null || accepts.isEmpty()) {
            throw new IllegalStateException(
                    "Server returned 402 with no payment options");
        }
        PaymentRequirements picked = selector.select(x402Version, accepts);
        if (picked == null) {
            throw new IllegalStateException(
                    "PaymentRequirementsSelector returned null");
        }
        return picked;
    }
}
