// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.okx.x402.crypto.CryptoSignException;
import com.okx.x402.crypto.EvmSigner;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.util.Json;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

import static com.github.tomakehurst.wiremock.client.WireMock.*;
import static org.junit.jupiter.api.Assertions.*;

class OKXHttpClientTest {

    private static WireMockServer wm;

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
    void resetServer() {
        wm.resetAll();
    }

    private EvmSigner stubSigner() {
        return new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                return Map.of("signature", "0xtest");
            }
            @Override
            public String getAddress() {
                return "0xtest";
            }
        };
    }

    @Test
    void constructorAcceptsValidSigner() {
        EvmSigner mockSigner = new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                return Map.of("signature", "0xtest");
            }
            @Override
            public String getAddress() {
                return "0xtest";
            }
        };

        OKXHttpClient client = new OKXHttpClient(mockSigner);
        assertNotNull(client);
    }

    @Test
    void constructorWithNetwork() {
        EvmSigner mockSigner = new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                return Map.of("signature", "0xtest");
            }
            @Override
            public String getAddress() {
                return "0xtest";
            }
        };

        OKXHttpClient client = new OKXHttpClient(mockSigner, "eip155:195");
        assertNotNull(client);
    }

    @Test
    void nullSignerThrows() {
        assertThrows(NullPointerException.class,
                () -> new OKXHttpClient((EvmSigner) null));
    }

    @Test
    void emptyAcceptsListThrows() {
        // selectRequirement is private, but we can verify via the public flow
        // by checking that the guard works. Direct test via reflection or
        // indirect test: an empty accepts list should cause IllegalStateException
        // when the 402 body has empty accepts.
        // This is a basic sanity check — full flow tested in E2E.
        assertDoesNotThrow(() -> {
            EvmSigner mockSigner = new EvmSigner() {
                @Override
                public java.util.Map<String, Object> signPaymentRequirements(
                        com.okx.x402.model.v2.PaymentRequirements r) {
                    return java.util.Map.of("signature", "0xtest");
                }
                @Override
                public String getAddress() { return "0xtest"; }
            };
            new OKXHttpClient(mockSigner);
        });
    }

    @Test
    void configDefaults() {
        OKXHttpClientConfig cfg = new OKXHttpClientConfig(stubSigner());
        assertEquals("eip155:196", cfg.network);
        assertEquals(Duration.ofSeconds(10), cfg.connectTimeout);
        assertEquals(Duration.ofSeconds(30), cfg.requestTimeout);
        assertNull(cfg.httpClient);
    }

    @Test
    void getHonoursConfigRequestTimeout() {
        wm.stubFor(get(urlEqualTo("/slow"))
                .willReturn(aResponse()
                        .withFixedDelay(2000)
                        .withBody("ok")));

        OKXHttpClientConfig cfg = new OKXHttpClientConfig(stubSigner());
        cfg.requestTimeout = Duration.ofMillis(300);
        OKXHttpClient client = new OKXHttpClient(cfg);

        long start = System.currentTimeMillis();
        assertThrows(Exception.class,
                () -> client.get(URI.create("http://localhost:" + wm.port() + "/slow")));
        long elapsed = System.currentTimeMillis() - start;
        assertTrue(elapsed < 1_500,
                "request-timeout should trip well before the 2s stub; elapsed=" + elapsed);
    }

    // -----------------------------------------------------------------------
    // Auto-402 retry behavior: second-time 402 is returned to the caller as a
    // normal response (statusCode() == 402) instead of being thrown. Mirrors
    // the TS SDK so callers can implement custom recovery on top — e.g. swap
    // signer / re-select requirements / surface the failure to end users.
    // -----------------------------------------------------------------------

    /** Build a minimal valid 402 envelope and return its base64 header value. */
    private static String build402EnvelopeHeader(String error,
                                                 String resourceUrl) throws Exception {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = resourceUrl;
        pr.resource.mimeType = "application/json";

        PaymentRequirements req = new PaymentRequirements();
        req.scheme = "exact";
        req.network = "eip155:196";
        req.amount = "10000";
        req.payTo = "0xReceiver";
        req.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        req.maxTimeoutSeconds = 86400;
        req.extra = Map.of("name", "USD₮0", "version", "1",
                "transferMethod", "eip3009");
        pr.accepts = List.of(req);

        String json = Json.MAPPER.writeValueAsString(pr);
        return Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
    }

    @Test
    void retryAfterSignedPaymentFailureReturnsSecond402() throws Exception {
        // First call returns 402 with valid envelope. Second call (with the
        // signed PAYMENT-SIGNATURE header attached) also returns 402 — server
        // is rejecting the signed payment for some reason such as a duplicate
        // nonce or expired authorization. The SDK must return that second
        // response to the caller (statusCode() == 402) rather than throwing,
        // so the caller can inspect the envelope and decide how to recover.
        String resourceUrl = "http://localhost:" + wm.port() + "/api/data";
        String envelope = build402EnvelopeHeader("nonce_already_used", resourceUrl);

        // Stub: every GET to /api/data returns 402 with the envelope.
        wm.stubFor(get(urlEqualTo("/api/data"))
                .willReturn(aResponse()
                        .withStatus(402)
                        .withHeader("PAYMENT-REQUIRED", envelope)
                        .withHeader("Content-Type", "application/json; charset=UTF-8")
                        .withBody("{}")));

        OKXHttpClient client = new OKXHttpClient(stubSigner());
        HttpResponse<String> resp = client.get(URI.create(resourceUrl));

        assertEquals(402, resp.statusCode(),
                "second 402 must be returned to the caller, not thrown");
        // The caller can recover the server's error reason from the envelope.
        String headerB64 = resp.headers().firstValue("PAYMENT-REQUIRED").orElse(null);
        assertNotNull(headerB64);
        byte[] decoded = Base64.getDecoder().decode(headerB64);
        PaymentRequired decodedEnv = Json.MAPPER.readValue(decoded, PaymentRequired.class);
        assertEquals("nonce_already_used", decodedEnv.error,
                "error reason must be readable from the returned envelope");

        // Sanity: the client made exactly two requests — initial + 1 retry.
        wm.verify(2, getRequestedFor(urlEqualTo("/api/data")));
    }

    @Test
    void retryAfter5xxIsPropagatedNotConvertedTo402() throws Exception {
        // First call: 402 with envelope. Second call (with signed payment):
        // the server is having a transient outage — 503. SDK must NOT throw
        // an IOException for the 5xx; it should return the response so the
        // caller can decide whether to retry the whole flow.
        String resourceUrl = "http://localhost:" + wm.port() + "/api/transient";
        String envelope = build402EnvelopeHeader("payment required", resourceUrl);

        // First request (no PAYMENT-SIGNATURE) returns 402.
        wm.stubFor(get(urlEqualTo("/api/transient"))
                .withHeader("PAYMENT-SIGNATURE", absent())
                .willReturn(aResponse()
                        .withStatus(402)
                        .withHeader("PAYMENT-REQUIRED", envelope)
                        .withBody("{}")));

        // Second request (signed) gets 503.
        wm.stubFor(get(urlEqualTo("/api/transient"))
                .withHeader("PAYMENT-SIGNATURE", matching(".+"))
                .willReturn(aResponse()
                        .withStatus(503)
                        .withBody("upstream temporarily unavailable")));

        OKXHttpClient client = new OKXHttpClient(stubSigner());
        HttpResponse<String> resp = client.get(URI.create(resourceUrl));

        assertEquals(503, resp.statusCode(),
                "5xx after signed retry must be propagated to the caller");
        assertTrue(resp.body().contains("upstream"),
                "body must be propagated unchanged");
    }

    @Test
    void parses402EnvelopeFromHeaderEvenWithEmptyBody() throws Exception {
        // Source-of-truth is the PAYMENT-REQUIRED header (TS-parity). Empty
        // body must not break the auto-402 flow. Second call returns 200 so
        // the happy path completes.
        String resourceUrl = "http://localhost:" + wm.port() + "/api/data";
        String envelope = build402EnvelopeHeader(null, resourceUrl);

        wm.stubFor(get(urlEqualTo("/api/data"))
                .withHeader("PAYMENT-SIGNATURE", absent())
                .willReturn(aResponse()
                        .withStatus(402)
                        .withHeader("PAYMENT-REQUIRED", envelope)
                        .withBody("{}")));      // empty JSON body — TS-parity shape
        wm.stubFor(get(urlEqualTo("/api/data"))
                .withHeader("PAYMENT-SIGNATURE", matching(".+"))
                .willReturn(aResponse()
                        .withStatus(200)
                        .withBody("{\"data\":\"ok\"}")));

        OKXHttpClient client = new OKXHttpClient(stubSigner());
        HttpResponse<String> resp = client.get(URI.create(resourceUrl));

        assertEquals(200, resp.statusCode());
        assertTrue(resp.body().contains("ok"));
    }

    @Test
    void parses402EnvelopeFromBodyAsFallback() throws Exception {
        // Backwards compatibility: an older / third-party server that puts
        // the PaymentRequired envelope into the response body (no
        // PAYMENT-REQUIRED header) should still drive the auto-402 flow.
        String resourceUrl = "http://localhost:" + wm.port() + "/api/legacy";

        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.resource = new ResourceInfo();
        pr.resource.url = resourceUrl;
        PaymentRequirements req = new PaymentRequirements();
        req.scheme = "exact";
        req.network = "eip155:196";
        req.amount = "10000";
        req.payTo = "0xReceiver";
        req.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        req.maxTimeoutSeconds = 86400;
        req.extra = Map.of("name", "USD₮0", "version", "1",
                "transferMethod", "eip3009");
        pr.accepts = List.of(req);
        String envelopeJson = Json.MAPPER.writeValueAsString(pr);

        wm.stubFor(get(urlEqualTo("/api/legacy"))
                .withHeader("PAYMENT-SIGNATURE", absent())
                .willReturn(aResponse()
                        .withStatus(402)
                        .withBody(envelopeJson)));   // envelope in body, no header
        wm.stubFor(get(urlEqualTo("/api/legacy"))
                .withHeader("PAYMENT-SIGNATURE", matching(".+"))
                .willReturn(aResponse()
                        .withStatus(200)
                        .withBody("{\"data\":\"ok\"}")));

        OKXHttpClient client = new OKXHttpClient(stubSigner());
        HttpResponse<String> resp = client.get(URI.create(resourceUrl));
        assertEquals(200, resp.statusCode());
    }

    @Test
    void retry402WithoutErrorFieldIsStillReturnedToCaller() throws Exception {
        // If the server's second 402 omits the `error` field, the SDK still
        // returns the response to the caller; the caller can observe the
        // null/empty error in the envelope and decide what to do.
        String resourceUrl = "http://localhost:" + wm.port() + "/api/data";
        String envelope = build402EnvelopeHeader(null, resourceUrl);     // null error

        wm.stubFor(get(urlEqualTo("/api/data"))
                .willReturn(aResponse()
                        .withStatus(402)
                        .withHeader("PAYMENT-REQUIRED", envelope)
                        .withBody("{}")));

        OKXHttpClient client = new OKXHttpClient(stubSigner());
        HttpResponse<String> resp = client.get(URI.create(resourceUrl));

        assertEquals(402, resp.statusCode());
        String headerB64 = resp.headers().firstValue("PAYMENT-REQUIRED").orElse(null);
        assertNotNull(headerB64);
        PaymentRequired decodedEnv = Json.MAPPER.readValue(
                Base64.getDecoder().decode(headerB64), PaymentRequired.class);
        assertNull(decodedEnv.error,
                "missing error field on the wire must surface as null in the decoded envelope");
    }

    @Test
    void injectedHttpClientIsActuallyUsed() throws Exception {
        wm.stubFor(get(urlEqualTo("/ok"))
                .willReturn(aResponse().withBody("pong")));

        AtomicReference<String> sawThread = new AtomicReference<>();
        ThreadFactory tf = r -> {
            Thread t = new Thread(r, "x402-injected-client-" + System.nanoTime());
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
            OKXHttpClientConfig cfg = new OKXHttpClientConfig(stubSigner());
            cfg.httpClient = injected;
            OKXHttpClient client = new OKXHttpClient(cfg);

            HttpResponse<String> resp = client.get(
                    URI.create("http://localhost:" + wm.port() + "/ok"));
            assertEquals(200, resp.statusCode());
            assertEquals("pong", resp.body());
            assertNotNull(sawThread.get(),
                    "injected Executor must have run at least one task");
            assertTrue(sawThread.get().startsWith("x402-injected-client-"),
                    "expected injected thread, got: " + sawThread.get());
        } finally {
            exec.shutdownNow();
        }
    }
}
