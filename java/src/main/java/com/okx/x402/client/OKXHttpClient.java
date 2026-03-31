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

    /**
     * Creates an OKXHttpClient with default X Layer mainnet network.
     *
     * @param signer EVM signer for payment authorization
     */
    public OKXHttpClient(EvmSigner signer) {
        this(signer, "eip155:196");
    }

    /**
     * Creates an OKXHttpClient with the specified network.
     *
     * @param signer EVM signer for payment authorization
     * @param network CAIP-2 network identifier
     */
    public OKXHttpClient(EvmSigner signer, String network) {
        this.signer = Objects.requireNonNull(signer);
        this.network = network;
        this.http = HttpClient.newHttpClient();
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
        HttpRequest initial = HttpRequest.newBuilder().uri(uri).GET().build();
        return request(initial);
    }

    /**
     * Generic request with auto 402 handling.
     *
     * @param initial the initial HTTP request
     * @return HTTP response after handling any 402
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    public HttpResponse<String> request(HttpRequest initial)
            throws IOException, InterruptedException {
        HttpResponse<String> resp = http.send(initial, HttpResponse.BodyHandlers.ofString());

        if (resp.statusCode() != 402) {
            return resp;
        }

        // Parse 402 body
        PaymentRequired paymentRequired = Json.MAPPER.readValue(
                resp.body(), PaymentRequired.class);

        // Select matching requirement
        PaymentRequirements selected = selectRequirement(paymentRequired.accepts);

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

        // Retry with PAYMENT-SIGNATURE
        String header = pp.toHeader();
        HttpRequest retry = HttpRequest.newBuilder()
                .uri(initial.uri())
                .header("PAYMENT-SIGNATURE", header)
                .GET()
                .build();

        return http.send(retry, HttpResponse.BodyHandlers.ofString());
    }

    private PaymentRequirements selectRequirement(java.util.List<PaymentRequirements> accepts) {
        return accepts.stream()
                .filter(r -> r.network.equals(network))
                .findFirst()
                .orElse(accepts.get(0));
    }
}
