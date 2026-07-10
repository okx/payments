// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.okx.x402.crypto.EvmSigner;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.util.Json;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.net.URI;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;

import static com.github.tomakehurst.wiremock.client.WireMock.*;
import static org.junit.jupiter.api.Assertions.*;

/**
 * Verifies the pluggable {@link PaymentRequirementsSelector} on
 * {@link OKXHttpClient}. Covers:
 * <ul>
 *   <li>default selector — network match beats first-fallback</li>
 *   <li>default selector — first fallback when no network matches</li>
 *   <li>custom selector receives all accepts unfiltered and its pick is used
 *       on the retry</li>
 * </ul>
 */
class PaymentRequirementsSelectorTest {

    private static final String USDT = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
    private static final String USDG = "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8";

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
    void reset() {
        wm.resetAll();
    }

    private EvmSigner captureSigner(AtomicReference<PaymentRequirements> sink) {
        return new EvmSigner() {
            @Override
            public Map<String, Object> signPaymentRequirements(PaymentRequirements r) {
                sink.set(r);
                return Map.of("signature", "0xsigned");
            }
            @Override
            public String getAddress() {
                return "0xabc";
            }
        };
    }

    private void stub402WithBothOptions() throws Exception {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.resource = new ResourceInfo();
        pr.resource.url = "http://localhost:" + wm.port() + "/api/data";
        pr.resource.mimeType = "application/json";

        PaymentRequirements usdt = new PaymentRequirements();
        usdt.scheme = "exact";
        usdt.network = "eip155:196";
        usdt.asset = USDT;
        usdt.amount = "10000";
        usdt.payTo = "0xReceiver";
        usdt.maxTimeoutSeconds = 86400;
        usdt.extra = Map.of("name", "USD\u20AE0", "version", "1");

        PaymentRequirements usdg = new PaymentRequirements();
        usdg.scheme = "exact";
        usdg.network = "eip155:196";
        usdg.asset = USDG;
        usdg.amount = "10000";
        usdg.payTo = "0xReceiver";
        usdg.maxTimeoutSeconds = 86400;
        usdg.extra = Map.of("name", "USDG", "version", "2");

        pr.accepts = List.of(usdt, usdg);

        String body = Json.MAPPER.writeValueAsString(pr);
        String encoded = Base64.getEncoder().encodeToString(body.getBytes());

        wm.stubFor(get(urlEqualTo("/api/data"))
                .inScenario("flow")
                .whenScenarioStateIs(com.github.tomakehurst.wiremock.stubbing.Scenario.STARTED)
                .willReturn(aResponse()
                        .withStatus(402)
                        .withHeader("PAYMENT-REQUIRED", encoded)
                        .withHeader("Content-Type", "application/json")
                        .withBody(body))
                .willSetStateTo("paid"));

        wm.stubFor(get(urlEqualTo("/api/data"))
                .inScenario("flow")
                .whenScenarioStateIs("paid")
                .willReturn(aResponse()
                        .withStatus(200)
                        .withBody("{\"ok\":true}")));
    }

    @Test
    void defaultSelectorPicksNetworkMatch() throws Exception {
        stub402WithBothOptions();
        AtomicReference<PaymentRequirements> signed = new AtomicReference<>();
        OKXHttpClient client = new OKXHttpClient(captureSigner(signed), "eip155:196");

        var resp = client.get(URI.create("http://localhost:" + wm.port() + "/api/data"));
        assertEquals(200, resp.statusCode());
        // First match wins — USDT is first in the list with a matching network
        assertEquals(USDT, signed.get().asset);
    }

    @Test
    void defaultSelectorFallsBackToFirstWhenNoNetworkMatches() throws Exception {
        stub402WithBothOptions();
        AtomicReference<PaymentRequirements> signed = new AtomicReference<>();
        OKXHttpClient client = new OKXHttpClient(captureSigner(signed), "eip155:999");

        var resp = client.get(URI.create("http://localhost:" + wm.port() + "/api/data"));
        assertEquals(200, resp.statusCode());
        assertEquals(USDT, signed.get().asset);   // first in list
    }

    @Test
    void customSelectorCanPreferUsdg() throws Exception {
        stub402WithBothOptions();
        AtomicReference<PaymentRequirements> signed = new AtomicReference<>();
        AtomicReference<List<PaymentRequirements>> seen = new AtomicReference<>();

        OKXHttpClientConfig cfg = new OKXHttpClientConfig(captureSigner(signed));
        cfg.paymentRequirementsSelector = (version, accepts) -> {
            seen.set(accepts);
            for (PaymentRequirements r : accepts) {
                if (USDG.equalsIgnoreCase(r.asset)) {
                    return r;
                }
            }
            return accepts.get(0);
        };
        OKXHttpClient client = new OKXHttpClient(cfg);

        var resp = client.get(URI.create("http://localhost:" + wm.port() + "/api/data"));
        assertEquals(200, resp.statusCode());
        assertEquals(USDG, signed.get().asset);
        assertEquals(2, seen.get().size(),
                "custom selector should receive the full accepts list");
    }

    @Test
    void defaultSelectorFactoryHandlesNullNetwork() {
        PaymentRequirements a = new PaymentRequirements();
        a.network = "eip155:1";
        PaymentRequirements b = new PaymentRequirements();
        b.network = "eip155:2";

        PaymentRequirementsSelector sel = PaymentRequirementsSelector.defaultSelector(null);
        assertSame(a, sel.select(2, List.of(a, b)));
    }
}
