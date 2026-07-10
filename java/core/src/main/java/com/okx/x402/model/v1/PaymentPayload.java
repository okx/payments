// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v1;

import com.okx.x402.util.Json;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Map;

/**
 * Represents a v1 payment payload that can be serialized to and from a header.
 */
public class PaymentPayload {

    /** The x402 protocol version. */
    public int x402Version;

    /** The payment scheme identifier. */
    public String scheme;

    /** The blockchain network. */
    public String network;

    /** The scheme-specific payload data. */
    public Map<String, Object> payload;

    /**
     * Encodes this payload as a Base64 header string.
     *
     * @return the Base64-encoded JSON representation
     */
    public String toHeader() {
        try {
            String json = Json.MAPPER.writeValueAsString(this);
            return Base64.getEncoder().encodeToString(
                    json.getBytes(StandardCharsets.UTF_8));
        } catch (IOException e) {
            throw new IllegalStateException(
                    "Unable to encode payment header", e);
        }
    }

    /**
     * Decodes a payment payload from a Base64 header string.
     *
     * @param header the Base64-encoded header
     * @return the decoded payment payload
     * @throws IOException if the header cannot be decoded
     */
    public static PaymentPayload fromHeader(String header) throws IOException {
        byte[] decoded = Base64.getDecoder().decode(header);
        return Json.MAPPER.readValue(decoded, PaymentPayload.class);
    }
}
