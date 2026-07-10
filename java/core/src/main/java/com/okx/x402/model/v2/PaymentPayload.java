// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import com.okx.x402.util.Json;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Map;

/** V2 payment payload sent via PAYMENT-SIGNATURE header. */
public class PaymentPayload {
    /** Protocol version. */
    public int x402Version = 2;
    /** Resource echo. */
    public ResourceInfo resource;
    /** Which accept[] item was chosen. */
    public PaymentRequirements accepted;
    /** Scheme-specific signed payload (e.g. ExactEIP3009Payload as map). */
    public Map<String, Object> payload;
    /** Optional extensions. */
    public Map<String, Object> extensions;

    /** Serialise and base64-encode for the PAYMENT-SIGNATURE header. */
    public String toHeader() {
        try {
            String json = Json.MAPPER.writeValueAsString(this);
            return Base64.getEncoder().encodeToString(json.getBytes(StandardCharsets.UTF_8));
        } catch (IOException e) {
            throw new IllegalStateException("Unable to encode payment header", e);
        }
    }

    /**
     * Decode from the PAYMENT-SIGNATURE header.
     *
     * @param header base64-encoded header string
     * @return decoded PaymentPayload
     * @throws IOException if decoding fails
     */
    public static PaymentPayload fromHeader(String header) throws IOException {
        byte[] decoded = Base64.getDecoder().decode(header);
        return Json.MAPPER.readValue(decoded, PaymentPayload.class);
    }
}
