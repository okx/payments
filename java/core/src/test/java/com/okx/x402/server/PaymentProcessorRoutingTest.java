// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;

import java.util.HashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

/**
 * Negative tests: path normalization (unpaid-bypass guard)
 * and the subscription-abort tri-classification (bare 402 vs 402 + PAYMENT-REQUIRED).
 */
class PaymentProcessorRoutingTest {

    // ------------------------------------------------------------------
    // normalizePath / matchRoute
    // ------------------------------------------------------------------

    @Test
    void normalizePathCollapsesVariants() {
        assertEquals("/premium", PaymentProcessor.normalizePath("/premium"));
        assertEquals("/premium", PaymentProcessor.normalizePath("/premium/"));
        assertEquals("/premium", PaymentProcessor.normalizePath("//premium"));
        assertEquals("/premium", PaymentProcessor.normalizePath("/premium?x=1"));
        assertEquals("/premium", PaymentProcessor.normalizePath("/premium/?x=1#frag"));
        assertEquals("/a/b", PaymentProcessor.normalizePath("/a//b/"));
        assertEquals("/", PaymentProcessor.normalizePath("/"));
        assertEquals("/", PaymentProcessor.normalizePath(""));
    }

    @Test
    void uriVariantsCannotBypassRouteMatch() {
        PaymentProcessor.RouteConfig config = new PaymentProcessor.RouteConfig();
        Map<String, PaymentProcessor.RouteConfig> routes = new HashMap<>();
        routes.put("GET /premium", config);
        PaymentProcessor processor =
                new PaymentProcessor(mock(FacilitatorClient.class), routes);

        assertSame(config, processor.matchRoute(request("GET", "/premium")));
        // These three variants must not miss the exact-string match — a miss means
        // PASS_THROUGH → free content whenever the downstream framework still serves the
        // resource.
        assertSame(config, processor.matchRoute(request("GET", "/premium/")));
        assertSame(config, processor.matchRoute(request("GET", "//premium")));
        assertSame(config, processor.matchRoute(request("GET", "/premium?x=1")));
        assertNull(processor.matchRoute(request("GET", "/other")));
    }

    // ------------------------------------------------------------------
    // subscription abort tri-classification
    // ------------------------------------------------------------------

    @Test
    void unauthorizedAbortAnswersBare402WithoutOffers() throws Exception {
        PaymentProcessor processor = subscriptionProcessor();
        processor.onProtectedRequest((req, cfg) ->
                PaymentHooks.ProtectedRequestResult.abort("invalid access proof",
                        PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED));

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/sub"), response));

        verify(response).setStatus(X402Response.SC_PAYMENT_REQUIRED);
        // Bare 402: NO PAYMENT-REQUIRED envelope — an agent must not be told to (re)pay.
        verify(response, never()).setHeader(eq("PAYMENT-REQUIRED"), anyString());
        ArgumentCaptor<String> body = ArgumentCaptor.forClass(String.class);
        verify(response).writeBody(body.capture());
        assertTrue(body.getValue().contains("invalid access proof"));
    }

    @Test
    void deniedAbortAnswersBare402WithoutOffers() throws Exception {
        PaymentProcessor processor = subscriptionProcessor();
        processor.onProtectedRequest((req, cfg) ->
                PaymentHooks.ProtectedRequestResult.abort("access_denied_by_merchant",
                        PaymentHooks.ProtectedRequestResult.AbortClass.DENIED));

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/sub"), response));

        verify(response, never()).setHeader(eq("PAYMENT-REQUIRED"), anyString());
    }

    @Test
    void notEligibleAbortCarriesOffersEnvelope() throws Exception {
        // Default abort class (period unpaid / plan mismatch): paying fixes it, so the 402
        // MUST carry PAYMENT-REQUIRED.
        PaymentProcessor processor = subscriptionProcessor();
        processor.onProtectedRequest((req, cfg) ->
                PaymentHooks.ProtectedRequestResult.abort("subscription_period_unpaid"));

        X402Response response = mock(X402Response.class);
        assertNull(processor.preHandle(request("GET", "/sub"), response));

        verify(response).setStatus(X402Response.SC_PAYMENT_REQUIRED);
        verify(response).setHeader(eq("PAYMENT-REQUIRED"), anyString());
    }

    // ------------------------------------------------------------------
    // warn-and-skip per accept option (one bad option ≠ route-wide 500)
    // ------------------------------------------------------------------

    @Test
    void badAcceptOptionIsSkippedNotFatal() {
        PaymentProcessor processor =
                new PaymentProcessor(mock(FacilitatorClient.class), new HashMap<>());
        AcceptOption good = AcceptOption.builder().scheme("period").network("eip155:196")
                .payTo("0x2222222222222222222222222222222222222222").price("$0.01").build();
        AcceptOption bad = AcceptOption.builder().scheme("period").network("eip155:424242")
                .payTo("0x2222222222222222222222222222222222222222").price("$0.02").build();

        PaymentProcessor.RouteConfig config = new PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>(java.util.List.of(good, bad));
        var out = processor.buildRequirementsList(request("GET", "/sub"), config);
        assertEquals(1, out.size());
        assertEquals("eip155:196", out.get(0).network);

        // Every option failing IS fatal — surfaced as a clean 500 by the 402 writers.
        config.accepts = new java.util.ArrayList<>(java.util.List.of(bad));
        PaymentProcessor.RouteConfig allBad = config;
        assertThrows(IllegalStateException.class,
                () -> processor.buildRequirementsList(request("GET", "/sub"), allBad));
    }

    private static PaymentProcessor subscriptionProcessor() {
        PaymentProcessor.RouteConfig config = new PaymentProcessor.RouteConfig();
        config.scheme = "period";
        config.network = "eip155:196";
        config.price = "$0.01";
        config.payTo = "0x2222222222222222222222222222222222222222";
        Map<String, PaymentProcessor.RouteConfig> routes = new HashMap<>();
        routes.put("GET /sub", config);
        return new PaymentProcessor(mock(FacilitatorClient.class), routes);
    }

    private static X402Request request(String method, String uri) {
        X402Request request = mock(X402Request.class);
        when(request.getMethod()).thenReturn(method);
        when(request.getRequestURI()).thenReturn(uri);
        when(request.getRequestURL()).thenReturn("https://seller.example" + uri);
        return request;
    }
}
