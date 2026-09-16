// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.model.v1.PaymentRequirements;
import com.github.tomakehurst.wiremock.WireMockServer;
import org.junit.jupiter.api.*;
import static com.github.tomakehurst.wiremock.client.WireMock.*;

import java.util.Set;

import static org.junit.jupiter.api.Assertions.*;

class HttpFacilitatorClientTest {

    static WireMockServer wm;
    HttpFacilitatorClient client;

    @BeforeAll
    static void startServer() {
        wm = new WireMockServer(0);
        wm.start();
    }

    @AfterAll
    static void stopServer() { wm.stop(); }

    @BeforeEach
    void setUp() {
        wm.resetAll();
        client = new HttpFacilitatorClient("http://localhost:" + wm.port());
    }

    @Test
    void verifyAndSettleHappyPath() throws Exception {
        wm.stubFor(post(urlEqualTo("/verify"))
            .willReturn(aResponse()
                .withHeader("Content-Type", "application/json")
                .withBody("{\"isValid\":true}")));

        wm.stubFor(post(urlEqualTo("/settle"))
            .willReturn(aResponse()
                .withHeader("Content-Type", "application/json")
                .withBody("{\"success\":true,\"txHash\":\"0xabc\",\"networkId\":\"1\"}")));

        PaymentRequirements req = new PaymentRequirements();
        VerificationResponse vr = client.verify("header", req);
        assertTrue(vr.isValid);

        SettlementResponse sr = client.settle("header", req);
        assertTrue(sr.success);
        assertEquals("0xabc", sr.txHash);
    }

    @Test
    void supportedEndpoint() throws Exception {
        wm.stubFor(get(urlEqualTo("/supported"))
            .willReturn(aResponse()
                .withHeader("Content-Type", "application/json")
                .withBody("{\"kinds\":[{\"scheme\":\"exact\",\"network\":\"base-sepolia\"}]}")));

        Set<Kind> kinds = client.supported();
        assertEquals(1, kinds.size());
    }

    @Test
    void verifyRejectsNon200Status() {
        PaymentRequirements req = new PaymentRequirements();

        wm.stubFor(post(urlEqualTo("/verify"))
            .willReturn(aResponse()
                .withStatus(500)
                .withBody("error")));

        Exception ex = assertThrows(Exception.class, () -> client.verify("header", req));
        assertTrue(ex.getMessage().contains("HTTP 500"));
    }
}
