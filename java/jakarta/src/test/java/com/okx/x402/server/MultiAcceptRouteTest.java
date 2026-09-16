// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.util.Json;
import jakarta.servlet.FilterChain;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;

import java.io.ByteArrayOutputStream;
import java.io.PrintWriter;
import java.util.Base64;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

/**
 * Covers the multi-accept route upgrade: a single route with multiple
 * {@link AcceptOption} entries produces a 402 envelope whose {@code accepts}
 * list contains one {@link PaymentRequirements} per option, and the matching
 * verify path still selects the client-picked option.
 */
class MultiAcceptRouteTest {

    private static final String USDT = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
    private static final String USDG = "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8";

    @Mock HttpServletRequest req;
    @Mock HttpServletResponse resp;
    @Mock FilterChain chain;
    @Mock FacilitatorClient fac;

    private PaymentFilter filter;

    @BeforeEach
    void init() throws Exception {
        MockitoAnnotations.openMocks(this);
        when(resp.getWriter()).thenReturn(new PrintWriter(new ByteArrayOutputStream(), true));

        PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
        route.network = "eip155:196";
        route.payTo = "0xReceiver";
        route.accepts = List.of(
                AcceptOption.builder()
                        .scheme("exact")
                        .price("$0.01")
                        .build(),          // USDT (default)
                AcceptOption.builder()
                        .scheme("exact")
                        .asset(USDG)
                        .price("$0.01")
                        .build()
        );

        filter = PaymentFilter.create(fac, Map.of("GET /api/data", route));
    }

    @Test
    void paymentRequiredEnvelopeListsBothOptions() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);

        ArgumentCaptor<String> headerValue = ArgumentCaptor.forClass(String.class);
        verify(resp).setHeader(eq("PAYMENT-REQUIRED"), headerValue.capture());

        String json = new String(Base64.getDecoder().decode(headerValue.getValue()));
        PaymentRequired pr = Json.MAPPER.readValue(json, PaymentRequired.class);

        assertEquals(2, pr.accepts.size(), "envelope should list both accept options");
        assertEquals(USDT.toLowerCase(), pr.accepts.get(0).asset.toLowerCase());
        assertEquals(USDG.toLowerCase(), pr.accepts.get(1).asset.toLowerCase());
        assertEquals("10000", pr.accepts.get(0).amount);
        assertEquals("10000", pr.accepts.get(1).amount);
        assertEquals("USD\u20AE0", pr.accepts.get(0).extra.get("name"));   // USDT EIP-712 name
        assertEquals("USDG", pr.accepts.get(1).extra.get("name"));
        assertEquals("1", pr.accepts.get(0).extra.get("version"));
        assertEquals("2", pr.accepts.get(1).extra.get("version"));
        for (PaymentRequirements r : pr.accepts) {
            assertEquals("eip155:196", r.network);
            assertEquals("0xReceiver", r.payTo);
            assertEquals("exact", r.scheme);
            assertEquals(86400, r.maxTimeoutSeconds);
        }
    }

    @Test
    void verifyUsesClientSelectedOption() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        // Client submits payload pointing at USDG option, echoing the
        // server-issued amount and payTo verbatim (as OKXHttpClient does).
        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDG;
        picked.payTo = "0xReceiver";
        picked.amount = "10000";

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(HttpServletResponse.SC_OK);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xtx";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        // Capture the PaymentRequirements passed to the facilitator — it must
        // be the server's canonical USDG entry (not what the client sent).
        ArgumentCaptor<PaymentRequirements> verified =
                ArgumentCaptor.forClass(PaymentRequirements.class);
        verify(fac).verify(any(), verified.capture());
        assertEquals(USDG.toLowerCase(), verified.getValue().asset.toLowerCase());
        assertEquals("10000", verified.getValue().amount);
        assertEquals("USDG", verified.getValue().extra.get("name"));

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp, never()).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void payloadWithUnmatchedOptionReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "aggr_deferred";        // not offered
        picked.network = "eip155:196";
        picked.asset = USDT;

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verifyNoInteractions(fac);
    }

    @Test
    void payloadWithDifferentPayToReturns402() throws Exception {
        // Client tries to redirect payment to its own address by tampering
        // with `accepted.payTo`. Server's strict match must reject this with
        // a 402 (no matching payment option) rather than silently letting
        // the verify call go through against the server's canonical payTo.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0xAttacker";              // <-- not the configured 0xReceiver

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verifyNoInteractions(fac);
    }

    @Test
    void payloadPayToCaseMismatchStillMatches() throws Exception {
        // EVM addresses are commonly written in mixed checksum case. The
        // server's payTo "0xReceiver" must match a client-side
        // "0xRECEIVER" — case-insensitive comparison, like asset.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0XRECEIVER";
        picked.amount = "10000";

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(HttpServletResponse.SC_OK);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xtx";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(fac).verify(any(), any());
        verify(resp, never()).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void payloadWithTamperedAmountReturns402() throws Exception {
        // A malicious / buggy client echoes the server's scheme/network/asset/payTo
        // verbatim but signs a smaller amount. The server-side strict matcher
        // must reject this with 402 — relying on the facilitator to catch it
        // is not a guarantee for all deployments.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0xReceiver";
        picked.amount = "1";                       // server expects "10000"

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verifyNoInteractions(fac);
    }

    @Test
    void payloadOmittingAmountReturns402() throws Exception {
        // amount is the only on-the-wire defence against price tampering, so
        // a payload that omits it (Jackson default null) must be rejected even
        // if every other field matches.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0xReceiver";
        // picked.amount intentionally left null

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verifyNoInteractions(fac);
    }

    @Test
    void payloadWithMaxTimeoutSecondsMismatchReturns402() throws Exception {
        // When the client sets a non-zero maxTimeoutSeconds it must match the
        // server's value. Setting it to 0 (Jackson default for absent field)
        // is tolerated — see payloadOmittingMaxTimeoutSecondsStillMatches.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0xReceiver";
        picked.amount = "10000";
        picked.maxTimeoutSeconds = 60;             // server defaults to 86400

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verifyNoInteractions(fac);
    }

    @Test
    void payloadOmittingMaxTimeoutSecondsStillMatches() throws Exception {
        // A v1-shaped client that omits maxTimeoutSeconds (Jackson default 0)
        // is tolerated; the server's authoritative value carries through.
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/data");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/api/data"));

        PaymentRequirements picked = new PaymentRequirements();
        picked.scheme = "exact";
        picked.network = "eip155:196";
        picked.asset = USDT;
        picked.payTo = "0xReceiver";
        picked.amount = "10000";
        // picked.maxTimeoutSeconds = 0 (primitive default — treat as "not set")

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/api/data";
        p.accepted = picked;
        p.payload = Map.of("signature", "0xdeadbeef");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(HttpServletResponse.SC_OK);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xtx";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(fac).verify(any(), any());
        verify(resp, never()).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void legacyScalarRouteStillEmitsSingleAccept() throws Exception {
        PaymentProcessor.RouteConfig legacy = new PaymentProcessor.RouteConfig();
        legacy.network = "eip155:196";
        legacy.payTo = "0xReceiver";
        legacy.price = "$0.01";
        PaymentFilter legacyFilter = PaymentFilter.create(fac,
                Map.of("GET /legacy", legacy));

        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/legacy");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/legacy"));

        legacyFilter.doFilter(req, resp, chain);

        ArgumentCaptor<String> headerValue = ArgumentCaptor.forClass(String.class);
        verify(resp).setHeader(eq("PAYMENT-REQUIRED"), headerValue.capture());

        String json = new String(Base64.getDecoder().decode(headerValue.getValue()));
        PaymentRequired pr = Json.MAPPER.readValue(json, PaymentRequired.class);

        assertEquals(1, pr.accepts.size(),
                "legacy RouteConfig must still produce exactly one accept entry");
        assertEquals(USDT.toLowerCase(), pr.accepts.get(0).asset.toLowerCase());
    }
}
