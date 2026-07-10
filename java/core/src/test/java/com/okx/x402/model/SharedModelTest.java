// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model;

import com.okx.x402.util.Json;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

class SharedModelTest {

    @Test
    void authorizationFields() {
        Authorization auth = new Authorization();
        auth.from = "0xFrom";
        auth.to = "0xTo";
        auth.value = "1000";
        auth.validAfter = "0";
        auth.validBefore = "999";
        auth.nonce = "0xnonce";

        assertEquals("0xFrom", auth.from);
        assertEquals("0xTo", auth.to);
        assertEquals("1000", auth.value);
    }

    @Test
    void authorizationJsonRoundTrip() throws Exception {
        Authorization auth = new Authorization();
        auth.from = "0xFrom";
        auth.to = "0xTo";
        auth.value = "500";

        String json = Json.MAPPER.writeValueAsString(auth);
        Authorization decoded = Json.MAPPER.readValue(json, Authorization.class);
        assertEquals("0xFrom", decoded.from);
        assertEquals("0xTo", decoded.to);
        assertEquals("500", decoded.value);
    }

    @Test
    void exactSchemePayloadFields() {
        ExactSchemePayload esp = new ExactSchemePayload();
        esp.signature = "0xSig";
        esp.authorization = new Authorization();
        esp.authorization.from = "0xPayer";

        assertEquals("0xSig", esp.signature);
        assertEquals("0xPayer", esp.authorization.from);
    }

    @Test
    void exactSchemePayloadJsonRoundTrip() throws Exception {
        ExactSchemePayload esp = new ExactSchemePayload();
        esp.signature = "0xSig";
        esp.authorization = new Authorization();
        esp.authorization.from = "0xPayer";

        String json = Json.MAPPER.writeValueAsString(esp);
        ExactSchemePayload decoded = Json.MAPPER.readValue(json, ExactSchemePayload.class);
        assertEquals("0xSig", decoded.signature);
        assertEquals("0xPayer", decoded.authorization.from);
    }

    @Test
    void settlementResponseHeaderConstructor() {
        SettlementResponseHeader srh = new SettlementResponseHeader(
                true, "0xtx", "eip155:196", "0xPayer");
        assertTrue(srh.success);
        assertEquals("0xtx", srh.transaction);
        assertEquals("eip155:196", srh.network);
        assertEquals("0xPayer", srh.payer);
    }

    @Test
    void settlementResponseHeaderDefaultConstructor() {
        SettlementResponseHeader srh = new SettlementResponseHeader();
        assertFalse(srh.success);
        assertNull(srh.transaction);
    }

    @Test
    void settlementResponseHeaderJsonIncludesNulls() throws Exception {
        SettlementResponseHeader srh = new SettlementResponseHeader(true, null, null, null);
        String json = Json.MAPPER.writeValueAsString(srh);
        // @JsonInclude(ALWAYS) should include null fields
        assertTrue(json.contains("\"transaction\":null"));
        assertTrue(json.contains("\"payer\":null"));
    }
}
