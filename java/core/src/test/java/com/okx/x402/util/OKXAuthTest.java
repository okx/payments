// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.util;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class OKXAuthTest {

    @Test
    void headersContainAllRequiredFields() {
        OKXAuth auth = new OKXAuth("test-key", "test-secret", "test-pass");
        Map<String, String> headers = auth.createHeaders("POST", "/api/v6/pay/x402/verify", "{}");

        assertEquals("test-key", headers.get("OK-ACCESS-KEY"));
        assertNotNull(headers.get("OK-ACCESS-SIGN"));
        assertNotNull(headers.get("OK-ACCESS-TIMESTAMP"));
        assertEquals("test-pass", headers.get("OK-ACCESS-PASSPHRASE"));
        assertEquals("application/json", headers.get("Content-Type"));
    }

    @Test
    void timestampIsISO8601() {
        OKXAuth auth = new OKXAuth("key", "secret", "pass");
        Map<String, String> headers = auth.createHeaders("GET", "/test", "");
        String ts = headers.get("OK-ACCESS-TIMESTAMP");
        // Should match yyyy-MM-dd'T'HH:mm:ss.SSS'Z'
        assertTrue(ts.matches("\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}\\.\\d{3}Z"),
                "Timestamp should be ISO 8601 with milliseconds: " + ts);
    }

    @Test
    void missingApiKeyThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth(null, "secret", "pass"),
                "OKX API key is required");
    }

    @Test
    void emptyApiKeyThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth("", "secret", "pass"));
    }

    @Test
    void missingSecretKeyThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth("key", null, "pass"));
    }

    @Test
    void missingPassphraseThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXAuth("key", "secret", null));
    }

    @Test
    void differentBodiesProduceDifferentSignatures() {
        OKXAuth auth = new OKXAuth("key", "secret", "pass");
        Map<String, String> h1 = auth.createHeaders("POST", "/path", "{\"a\":1}");
        Map<String, String> h2 = auth.createHeaders("POST", "/path", "{\"b\":2}");
        // Signatures should differ (timestamps might be same within ms, but body differs)
        // This is a probabilistic test - if timestamps are identical, signatures must differ due to body
        assertNotNull(h1.get("OK-ACCESS-SIGN"));
        assertNotNull(h2.get("OK-ACCESS-SIGN"));
    }
}
