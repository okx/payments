// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import org.junit.jupiter.api.*;

import java.io.IOException;
import java.net.http.HttpClient;
import java.time.Duration;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicReference;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.LinkedBlockingQueue;

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

    @Test
    void retryOnHttp429() throws Exception {
        // First call returns 429, second succeeds
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .inScenario("retry429")
                .whenScenarioStateIs("Started")
                .willReturn(aResponse().withStatus(429).withBody("rate limited"))
                .willSetStateTo("retried"));

        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .inScenario("retry429")
                .whenScenarioStateIs("retried")
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true,\"payer\":\"0xretried\"}")));

        VerifyResponse vr = client.verify(new PaymentPayload(), new PaymentRequirements());
        assertTrue(vr.isValid);
        assertEquals("0xretried", vr.payer);
    }

    @Test
    void configDefaults() {
        OKXFacilitatorConfig cfg = new OKXFacilitatorConfig("k", "s", "p");
        assertEquals("https://www.okx.com", cfg.baseUrl);
        assertEquals(Duration.ofSeconds(10), cfg.connectTimeout);
        assertEquals(Duration.ofSeconds(30), cfg.requestTimeout);
        assertNull(cfg.httpClient);
    }

    @Test
    void customRequestTimeoutIsHonoured() {
        // Stub delays response 2s; client's request timeout is 300ms.
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withFixedDelay(2000)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true}")));

        OKXFacilitatorConfig cfg = new OKXFacilitatorConfig("k", "s", "p");
        cfg.baseUrl = "http://localhost:" + wm.port();
        cfg.requestTimeout = Duration.ofMillis(300);
        OKXFacilitatorClient fast = new OKXFacilitatorClient(cfg);

        long start = System.currentTimeMillis();
        assertThrows(Exception.class,
                () -> fast.verify(new PaymentPayload(), new PaymentRequirements()));
        long elapsed = System.currentTimeMillis() - start;
        // MAX_RETRIES=3 with exponential backoff up to 7s total; just
        // confirm we didn't wait for the 2s stub on every attempt.
        assertTrue(elapsed < 10_000,
                "request-timeout should trip well before the 2s stub; elapsed=" + elapsed);
    }

    @Test
    void customHttpExecutorOverridesJdkClient() throws Exception {
        // When both httpExecutor and httpClient are supplied, the executor wins.
        java.util.concurrent.atomic.AtomicInteger calls =
                new java.util.concurrent.atomic.AtomicInteger();
        HttpExecutor spy = (method, uri, body, headers, timeout) -> {
            calls.incrementAndGet();
            assertEquals("POST", method);
            assertTrue(uri.toString().endsWith("/api/v6/pay/x402/verify"));
            assertNotNull(headers.get("OK-ACCESS-KEY"));
            assertNotNull(timeout);
            return new HttpExecutor.HttpExecResult(
                    200, "{\"isValid\":true,\"payer\":\"0xspy\"}");
        };

        OKXFacilitatorConfig cfg = new OKXFacilitatorConfig("k", "s", "p");
        cfg.baseUrl = "http://unused";
        cfg.httpExecutor = spy;
        // httpClient is set but MUST be ignored in favour of the executor.
        cfg.httpClient = HttpClient.newHttpClient();
        OKXFacilitatorClient custom = new OKXFacilitatorClient(cfg);

        VerifyResponse vr = custom.verify(new PaymentPayload(), new PaymentRequirements());
        assertTrue(vr.isValid);
        assertEquals("0xspy", vr.payer);
        assertEquals(1, calls.get(), "spy executor must have been called exactly once");
    }

    @Test
    void customHttpExecutorRetryOn429() throws Exception {
        // 429 on first call, 200 on second — proves retry logic still works
        // when the executor is custom.
        java.util.concurrent.atomic.AtomicInteger attempts =
                new java.util.concurrent.atomic.AtomicInteger();
        HttpExecutor retrySpy = (method, uri, body, headers, timeout) -> {
            int n = attempts.incrementAndGet();
            if (n == 1) {
                return new HttpExecutor.HttpExecResult(429, "rate limited");
            }
            return new HttpExecutor.HttpExecResult(
                    200, "{\"isValid\":true,\"payer\":\"0xretried\"}");
        };

        OKXFacilitatorConfig cfg = new OKXFacilitatorConfig("k", "s", "p");
        cfg.baseUrl = "http://unused";
        cfg.httpExecutor = retrySpy;
        OKXFacilitatorClient custom = new OKXFacilitatorClient(cfg);

        VerifyResponse vr = custom.verify(new PaymentPayload(), new PaymentRequirements());
        assertTrue(vr.isValid);
        assertEquals("0xretried", vr.payer);
        assertEquals(2, attempts.get(), "executor should see two attempts (429 then 200)");
    }

    @Test
    void injectedHttpClientIsActuallyUsed() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true,\"payer\":\"0xinjected\"}")));

        // Build an HttpClient with a trackable Executor: threads are named
        // with a distinguishable prefix we can detect later.
        AtomicReference<String> sawThread = new AtomicReference<>();
        ThreadFactory tf = r -> {
            Thread t = new Thread(r, "x402-injected-pool-" + System.nanoTime());
            t.setDaemon(true);
            return t;
        };
        ThreadPoolExecutor exec = new ThreadPoolExecutor(
                2, 2, 60, TimeUnit.SECONDS, new LinkedBlockingQueue<>(), tf);
        HttpClient injected = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5))
                .executor(r -> exec.execute(() -> {
                    sawThread.compareAndSet(null, Thread.currentThread().getName());
                    r.run();
                }))
                .build();

        try {
            OKXFacilitatorConfig cfg = new OKXFacilitatorConfig("k", "s", "p");
            cfg.baseUrl = "http://localhost:" + wm.port();
            cfg.httpClient = injected;
            OKXFacilitatorClient custom = new OKXFacilitatorClient(cfg);

            VerifyResponse vr = custom.verify(new PaymentPayload(), new PaymentRequirements());
            assertTrue(vr.isValid);
            assertEquals("0xinjected", vr.payer);
            assertNotNull(sawThread.get(),
                    "injected Executor must have run at least one task");
            assertTrue(sawThread.get().startsWith("x402-injected-pool-"),
                    "expected injected thread, got: " + sawThread.get());
        } finally {
            exec.shutdownNow();
        }
    }

    @Test
    void retryOnOkx50011RateLimit() throws Exception {
        // First call returns OKX rate-limit envelope, second succeeds
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .inScenario("retry50011")
                .whenScenarioStateIs("Started")
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":\"50011\",\"msg\":\"Too many requests\",\"data\":{}}"))
                .willSetStateTo("retried"));

        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/verify"))
                .inScenario("retry50011")
                .whenScenarioStateIs("retried")
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"isValid\":true,\"payer\":\"0xretried\"}")));

        VerifyResponse vr = client.verify(new PaymentPayload(), new PaymentRequirements());
        assertTrue(vr.isValid);
        assertEquals("0xretried", vr.payer);
    }

}
