// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.model.v2.PaymentPayload;
import org.junit.jupiter.api.Test;

import java.util.HashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

/**
 * Unit tests for {@link PaymentProcessor#extractPayerFrom} — the claimed-payer
 * extraction used by the exempt-payer (fee waiver) whitelist check.
 */
class ExemptPayerFromTest {

    private static PaymentPayload payload(Map<String, Object> inner) {
        PaymentPayload pp = new PaymentPayload();
        pp.payload = inner;
        return pp;
    }

    @Test
    void eip3009AuthorizationFrom() {
        PaymentPayload pp = payload(Map.of(
                "signature", "0xsig",
                "authorization", Map.of("from", "0xPayer", "to", "0xSeller")));
        assertEquals("0xPayer", PaymentProcessor.extractPayerFrom(pp));
    }

    @Test
    void permit2AuthorizationFrom() {
        PaymentPayload pp = payload(Map.of(
                "signature", "0xsig",
                "permit2Authorization", Map.of("from", "0xPayer")));
        assertEquals("0xPayer", PaymentProcessor.extractPayerFrom(pp));
    }

    @Test
    void authorizationTakesPrecedenceOverPermit2() {
        PaymentPayload pp = payload(Map.of(
                "authorization", Map.of("from", "0xEoa"),
                "permit2Authorization", Map.of("from", "0xOther")));
        assertEquals("0xEoa", PaymentProcessor.extractPayerFrom(pp));
    }

    @Test
    void unknownSchemeShapeReturnsNull() {
        assertNull(PaymentProcessor.extractPayerFrom(
                payload(Map.of("signature", "0xsig"))));
    }

    @Test
    void nonMapAuthorizationReturnsNull() {
        assertNull(PaymentProcessor.extractPayerFrom(
                payload(Map.of("authorization", "not-a-map"))));
    }

    @Test
    void blankFromReturnsNull() {
        assertNull(PaymentProcessor.extractPayerFrom(
                payload(Map.of("authorization", Map.of("from", " ")))));
    }

    @Test
    void nullFromFieldReturnsNull() {
        Map<String, Object> auth = new HashMap<>();
        auth.put("from", null);
        assertNull(PaymentProcessor.extractPayerFrom(
                payload(Map.of("authorization", auth))));
    }

    @Test
    void nullPayloadReturnsNull() {
        assertNull(PaymentProcessor.extractPayerFrom(null));
        assertNull(PaymentProcessor.extractPayerFrom(new PaymentPayload()));
    }
}
