// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v1;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class V1ModelTest {

    @Test
    void paymentPayloadRoundTrip() throws Exception {
        PaymentPayload p = new PaymentPayload();
        p.x402Version = 1;
        p.scheme = "exact";
        p.network = "base-sepolia";
        p.payload = Map.of("resource", "/test", "amount", "100");

        String header = p.toHeader();
        assertNotNull(header);

        PaymentPayload decoded = PaymentPayload.fromHeader(header);
        assertEquals(1, decoded.x402Version);
        assertEquals("exact", decoded.scheme);
        assertEquals("base-sepolia", decoded.network);
        assertEquals("/test", decoded.payload.get("resource"));
    }

    @Test
    void paymentPayloadFromHeaderThrowsOnInvalid() {
        assertThrows(Exception.class, () -> PaymentPayload.fromHeader("!!!invalid!!!"));
    }

    @Test
    void paymentRequiredResponseDefaults() {
        PaymentRequiredResponse prr = new PaymentRequiredResponse();
        assertNotNull(prr.accepts);
        assertTrue(prr.accepts.isEmpty());
        assertEquals(0, prr.x402Version);
    }

    @Test
    void paymentRequirementsFields() {
        PaymentRequirements pr = new PaymentRequirements();
        pr.scheme = "exact";
        pr.network = "base-sepolia";
        pr.maxAmountRequired = "1000";
        pr.payTo = "0xAddr";
        pr.asset = "USDC";
        pr.maxTimeoutSeconds = 30;

        assertEquals("exact", pr.scheme);
        assertEquals("1000", pr.maxAmountRequired);
    }
}
