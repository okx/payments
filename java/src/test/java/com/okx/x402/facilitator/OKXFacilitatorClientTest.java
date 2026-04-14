package com.okx.x402.facilitator;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import org.junit.jupiter.api.*;

import java.io.IOException;

import static com.github.tomakehurst.wiremock.client.WireMock.*;
import static org.junit.jupiter.api.Assertions.*;

class OKXFacilitatorClientTest {

    static WireMockServer wm;
    OKXFacilitatorClient client;

    @BeforeAll
    static void startServer() {
        wm = new WireMockServer(0);
        wm.start();
    }

    @AfterAll
    static void stopServer() {
        wm.stop();
    }

    @BeforeEach
    void setUp() {
        wm.resetAll();
        client = new OKXFacilitatorClient("test-key", "test-secret", "test-pass",
                "http://localhost:" + wm.port());
    }

    @Test
    void verifyHappyPath() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true,\"payer\":\"0xabc\"}")));

        PaymentPayload payload = new PaymentPayload();
        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:196";

        VerifyResponse vr = client.verify(payload, req);
        assertTrue(vr.isValid);
        assertEquals("0xabc", vr.payer);
    }

    @Test
    void settleHappyPath() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/settle"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":true,\"transaction\":\"0xdef\",\"network\":\"eip155:196\"}")));

        PaymentPayload payload = new PaymentPayload();
        PaymentRequirements req = new PaymentRequirements();

        SettleResponse sr = client.settle(payload, req);
        assertTrue(sr.success);
        assertEquals("0xdef", sr.transaction);
        assertEquals("eip155:196", sr.network);
    }

    @Test
    void supportedHappyPath() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/supported"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"kinds\":[{\"scheme\":\"exact\",\"network\":\"eip155:196\"}],\"extensions\":[],\"signers\":{}}")));

        SupportedResponse sr = client.supported();
        assertEquals(1, sr.kinds.size());
        assertEquals("eip155:196", sr.kinds.get(0).network);
    }

    @Test
    void authHeadersPresent() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true}")));

        client.verify(new PaymentPayload(), new PaymentRequirements());

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/verify"))
                .withHeader("OK-ACCESS-KEY", equalTo("test-key"))
                .withHeader("OK-ACCESS-PASSPHRASE", equalTo("test-pass"))
                .withHeader("OK-ACCESS-SIGN", matching(".+"))
                .withHeader("OK-ACCESS-TIMESTAMP", matching(".+")));
    }

    @Test
    void errorCode50103MapsToReadableMessage() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withStatus(401)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":\"50103\",\"msg\":\"API key invalid\"}")));

        IOException ex = assertThrows(IOException.class,
                () -> client.verify(new PaymentPayload(), new PaymentRequirements()));
        assertTrue(ex.getMessage().contains("Invalid API key"));
    }

    @Test
    void missingCredentialsThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXFacilitatorClient(null, "secret", "pass"));
    }

    @Test
    void nonOkxErrorFormat() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withStatus(500)
                        .withBody("Internal Server Error")));

        assertThrows(IOException.class,
                () -> client.verify(new PaymentPayload(), new PaymentRequirements()));
    }

    @Test
    void settleSyncSettleTrueIncludesField() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/settle"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":true,\"transaction\":\"0xdef\","
                                + "\"network\":\"eip155:196\",\"status\":\"success\"}")));

        PaymentPayload payload = new PaymentPayload();
        PaymentRequirements req = new PaymentRequirements();

        SettleResponse sr = client.settle(payload, req, true);
        assertTrue(sr.success);
        assertEquals("success", sr.status);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/settle"))
                .withRequestBody(containing("\"syncSettle\":true")));
    }

    @Test
    void settleSyncSettleFalseOmitsField() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/settle"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":true,\"transaction\":\"0xdef\","
                                + "\"network\":\"eip155:196\",\"status\":\"pending\"}")));

        PaymentPayload payload = new PaymentPayload();
        PaymentRequirements req = new PaymentRequirements();

        SettleResponse sr = client.settle(payload, req, false);
        assertTrue(sr.success);
        assertEquals("pending", sr.status);

        // syncSettle should not be in the body when false
        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/settle"))
                .withRequestBody(notContaining("syncSettle")));
    }

    @Test
    void settleStatusHappyPath() throws Exception {
        wm.stubFor(get(urlPathEqualTo("/api/v6/pay/x402/settle/status"))
                .withQueryParam("txHash", equalTo("0xabc123"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":true,\"transaction\":\"0xabc123\","
                                + "\"network\":\"eip155:196\",\"status\":\"success\","
                                + "\"payer\":\"0xpayer\"}")));

        SettleResponse sr = client.settleStatus("0xabc123");
        assertTrue(sr.success);
        assertEquals("0xabc123", sr.transaction);
        assertEquals("success", sr.status);
        assertEquals("0xpayer", sr.payer);
    }

    @Test
    void settleStatusNotFound() throws Exception {
        wm.stubFor(get(urlPathEqualTo("/api/v6/pay/x402/settle/status"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"success\":false,\"errorReason\":\"not_found\","
                                + "\"errorMessage\":\"Transaction not found\"}")));

        SettleResponse sr = client.settleStatus("0xnonexistent");
        assertFalse(sr.success);
        assertEquals("not_found", sr.errorReason);
    }
}
