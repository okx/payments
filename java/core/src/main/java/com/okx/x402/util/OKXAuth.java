// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.util;

import java.nio.charset.StandardCharsets;
import java.security.InvalidKeyException;
import java.security.NoSuchAlgorithmException;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.Base64;
import java.util.Map;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

/**
 * OKX API authentication helper.
 * Generates HMAC-SHA256 signed headers per OKX API specification.
 */
public class OKXAuth {

    private final String apiKey;
    private final String secretKey;
    private final String passphrase;

    /**
     * Creates an OKXAuth instance with the given credentials.
     *
     * @param apiKey OKX API key
     * @param secretKey OKX secret key
     * @param passphrase OKX passphrase
     * @throws IllegalArgumentException if any credential is null or empty
     */
    public OKXAuth(String apiKey, String secretKey, String passphrase) {
        if (apiKey == null || apiKey.isEmpty()) {
            throw new IllegalArgumentException("OKX API key is required");
        }
        if (secretKey == null || secretKey.isEmpty()) {
            throw new IllegalArgumentException("OKX secret key is required");
        }
        if (passphrase == null || passphrase.isEmpty()) {
            throw new IllegalArgumentException("OKX passphrase is required");
        }
        this.apiKey = apiKey;
        this.secretKey = secretKey;
        this.passphrase = passphrase;
    }

    /**
     * Generate auth headers for an OKX API request.
     *
     * @param method HTTP method (GET, POST, etc.)
     * @param path API path (e.g. /api/v6/pay/x402/verify)
     * @param body request body (null or empty for GET)
     * @return map of auth headers
     */
    public Map<String, String> createHeaders(String method, String path, String body) {
        String timestamp = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'")
                .withZone(ZoneOffset.UTC)
                .format(Instant.now());
        String prehash = timestamp + method + path + (body != null ? body : "");
        String signature = Base64.getEncoder().encodeToString(
                hmacSha256(secretKey.getBytes(StandardCharsets.UTF_8),
                        prehash.getBytes(StandardCharsets.UTF_8)));

        return Map.of(
                "OK-ACCESS-KEY", apiKey,
                "OK-ACCESS-SIGN", signature,
                "OK-ACCESS-TIMESTAMP", timestamp,
                "OK-ACCESS-PASSPHRASE", passphrase,
                "Content-Type", "application/json"
        );
    }

    private static byte[] hmacSha256(byte[] key, byte[] data) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(key, "HmacSHA256"));
            return mac.doFinal(data);
        } catch (NoSuchAlgorithmException | InvalidKeyException e) {
            throw new IllegalStateException("HMAC-SHA256 computation failed", e);
        }
    }
}
