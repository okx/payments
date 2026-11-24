// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;

class PaymentPayloadV2Test {

    @Test
    void headerRoundTripMaintainsFields() throws Exception {
        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://example.com/api";
        p.resource.mimeType = "application/json";

        PaymentRequirements req = new PaymentRequirements();
        req.scheme = "exact";
        req.network = "eip155:196";
        req.amount = "10000";
        req.payTo = "0x1234";
        p.accepted = req;

        p.payload = Map.of("signature", "0xabc", "authorization", Map.of("from", "0x1"));

        String header = p.toHeader();
        assertNotNull(header);

        PaymentPayload decoded = PaymentPayload.fromHeader(header);
        assertEquals(2, decoded.x402Version);
        assertNotNull(decoded.resource);
        assertEquals("http://example.com/api", decoded.resource.url);
        assertNotNull(decoded.accepted);
        assertEquals("eip155:196", decoded.accepted.network);
        assertEquals("10000", decoded.accepted.amount);
        assertNotNull(decoded.payload);
        assertEquals("0xabc", decoded.payload.get("signature"));
    }

    @Test
    void fromHeaderThrowsOnInvalidBase64() {
        assertThrows(Exception.class, () -> PaymentPayload.fromHeader("not-valid-json!!!"));
    }
}
