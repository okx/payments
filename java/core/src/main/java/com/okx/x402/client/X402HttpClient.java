// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.crypto.CryptoSigner;
import com.okx.x402.crypto.CryptoSignException;
import com.okx.x402.model.v1.PaymentPayload;

import java.io.IOException;
import java.math.BigInteger;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.UUID;

/**
 * V1 HTTP client that builds requests with X-PAYMENT header.
 * Kept for backward compatibility.
 */
public class X402HttpClient {

    private final HttpClient http = HttpClient.newHttpClient();
    private final int x402Version = 1;
    private final String scheme = "exact";
    private final String network = "base-sepolia";

    private final CryptoSigner signer;

    /**
     * Creates a new X402 HTTP client with the specified crypto signer.
     *
     * @param signer the crypto signer for signing payment headers
     */
    public X402HttpClient(CryptoSigner signer) {
        this.signer = signer;
    }

    /**
     * Sends HTTP request, visible for test override.
     *
     * @param request the HTTP request to send
     * @return the HTTP response
     * @throws IOException if an I/O error occurs
     * @throws InterruptedException if the request is interrupted
     */
    protected HttpResponse<String> sendRequest(HttpRequest request)
            throws IOException, InterruptedException {
        return http.send(request, HttpResponse.BodyHandlers.ofString());
    }

    /**
     * Build and execute a GET request with X-PAYMENT header.
     *
     * @param uri destination URI
     * @param amount amount in atomic units
     * @param assetContract token contract address
     * @param payTo receiver address
     * @return HTTP response
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    public HttpResponse<String> get(URI uri,
                                    BigInteger amount,
                                    String assetContract,
                                    String payTo)
            throws IOException, InterruptedException {

        Map<String, Object> pl = new LinkedHashMap<>();
        pl.put("amount", amount.toString());
        pl.put("asset", assetContract);
        pl.put("payTo", payTo);
        pl.put("resource", uri.getPath());
        pl.put("nonce", UUID.randomUUID().toString());
        try {
            pl.put("signature", signer.sign(pl));
        } catch (CryptoSignException e) {
            throw new RuntimeException("Failed to sign payment payload", e);
        }

        PaymentPayload p = new PaymentPayload();
        p.x402Version = x402Version;
        p.scheme = scheme;
        p.network = network;
        p.payload = pl;

        HttpRequest req = HttpRequest.newBuilder()
                .uri(uri)
                .header("X-PAYMENT", p.toHeader())
                .GET()
                .build();

        return sendRequest(req);
    }
}
