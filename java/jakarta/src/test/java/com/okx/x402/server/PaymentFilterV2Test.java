// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.PaymentHooks;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletRequest;
import jakarta.servlet.ServletResponse;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintWriter;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

class PaymentFilterV2Test {

    @Mock HttpServletRequest req;
    @Mock HttpServletResponse resp;
    @Mock FilterChain chain;
    @Mock FacilitatorClient fac;

    private PaymentFilter filter;

    @BeforeEach
    void init() throws Exception {
        MockitoAnnotations.openMocks(this);
        when(resp.getWriter()).thenReturn(new PrintWriter(new ByteArrayOutputStream(), true));

        PaymentProcessor.RouteConfig xlayerConfig = new PaymentProcessor.RouteConfig();
        xlayerConfig.network = "eip155:196";
        xlayerConfig.payTo = "0xReceiver";
        xlayerConfig.price = "$0.01";

        filter = PaymentFilter.create(fac, Map.of(
                "GET /protected/xlayer-usdt", xlayerConfig
        ));
    }

    @Test
    void freeEndpointPassesThrough() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/public");

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp, never()).setStatus(anyInt());
    }

    @Test
    void missingHeaderReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void validV2HeaderVerifyAndSettle() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        // Build V2 payment payload
        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/protected/xlayer-usdt";
        p.payload = Map.of("signature", "0xabc");
        String header = p.toHeader();
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(header);

        // Facilitator verify succeeds
        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        // Handler returns 200
        when(resp.getStatus()).thenReturn(HttpServletResponse.SC_OK);

        // Settlement succeeds
        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xdef";
        sr.network = "eip155:196";
        sr.payer = "0xPayer";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp, never()).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(resp).setHeader(eq("PAYMENT-RESPONSE"), any());
        verify(resp).setHeader(eq("Access-Control-Expose-Headers"), eq("PAYMENT-RESPONSE"));
    }

    @Test
    void paymentRequiredHeaderSetOn402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(resp).setHeader(eq("PAYMENT-REQUIRED"), any());
        verify(resp).setHeader(eq("Access-Control-Expose-Headers"), eq("PAYMENT-REQUIRED"));
    }

    @Test
    void v1FallbackHeaderWorks() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("signature", "0xabc");
        String header = p.toHeader();
        when(req.getHeader("X-PAYMENT")).thenReturn(header);

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0x123";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);
        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
    }

    @Test
    void verificationExceptionReturns500() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        when(fac.verify(any(), any())).thenThrow(new IOException("Network error"));

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void settlementFailureReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = false;
        sr.errorReason = "insufficient_funds";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void nonHttpRequestPassesThrough() throws Exception {
        ServletRequest nonHttpReq = mock(ServletRequest.class);
        ServletResponse nonHttpRes = mock(ServletResponse.class);

        filter.doFilter(nonHttpReq, nonHttpRes, chain);

        verify(chain).doFilter(nonHttpReq, nonHttpRes);
        verifyNoInteractions(fac);
    }

    @Test
    void malformedHeaderReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn("not-valid-base64!!!");

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void errorResponseSkipsSettlement() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        // Handler returns 500
        when(resp.getStatus()).thenReturn(500);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(fac, never()).settle(any(), any(), anyBoolean());
    }

    @Test
    void routeLevelPricing() throws Exception {
        PaymentProcessor.RouteConfig cheapConfig = new PaymentProcessor.RouteConfig();
        cheapConfig.network = "eip155:196";
        cheapConfig.payTo = "0xReceiver";
        cheapConfig.price = "$0.01";

        PaymentProcessor.RouteConfig expensiveConfig = new PaymentProcessor.RouteConfig();
        expensiveConfig.network = "eip155:196";
        expensiveConfig.payTo = "0xReceiver";
        expensiveConfig.price = "$1.00";

        PaymentFilter multiFilter = PaymentFilter.create(fac, Map.of(
                "GET /cheap", cheapConfig,
                "GET /expensive", expensiveConfig
        ));

        // Just verify both routes are accepted (no exception)
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/cheap");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/cheap"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        multiFilter.doFilter(req, resp, chain);
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    // -----------------------------------------------------------------------
    // Settlement timeout polling + hook tests
    // -----------------------------------------------------------------------

    @Test
    void settleTimeoutPollsAndRecovers() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        pp.accepted = serverEchoedRequirements();
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        vr.payer = "0xPayer";
        when(fac.verify(any(), any())).thenReturn(vr);

        // settle returns timeout
        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xTimeoutTx";
        sr.network = "eip155:196";
        sr.status = "timeout";
        sr.payer = "0xPayer";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        // settleStatus poll returns success
        SettleResponse polled = new SettleResponse();
        polled.success = true;
        polled.transaction = "0xTimeoutTx";
        polled.network = "eip155:196";
        polled.status = "success";
        polled.payer = "0xPayer";
        when(fac.settleStatus("0xTimeoutTx")).thenReturn(polled);

        when(resp.getStatus()).thenReturn(200);

        filter.processor().pollDeadline(java.time.Duration.ofSeconds(3))
              .pollInterval(java.time.Duration.ofMillis(200));

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(fac).settleStatus("0xTimeoutTx");
        verify(resp).setHeader(eq("PAYMENT-RESPONSE"), any());
    }

    @Test
    void settleTimeoutHookGrantsAccess() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        pp.accepted = serverEchoedRequirements();
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xTimeoutTx";
        sr.network = "eip155:196";
        sr.status = "timeout";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        // Poll returns pending (never confirms)
        SettleResponse pending = new SettleResponse();
        pending.success = true;
        pending.status = "pending";
        when(fac.settleStatus("0xTimeoutTx")).thenReturn(pending);

        when(resp.getStatus()).thenReturn(200);

        // Hook grants access
        filter.processor().pollDeadline(java.time.Duration.ofMillis(500))
              .pollInterval(java.time.Duration.ofMillis(200))
              .onSettlementTimeout((txHash, network) ->
                      PaymentHooks.SettlementTimeoutResult.confirmed());

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp).setHeader(eq("PAYMENT-RESPONSE"), any());
    }

    // -----------------------------------------------------------------------
    // Lifecycle hook tests
    // -----------------------------------------------------------------------

    @Test
    void beforeVerifyHookCanAbort() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        filter.processor().onBeforeVerify((payload, requirements) ->
                PaymentHooks.AbortResult.abort("blocked_by_hook"));

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(fac, never()).verify(any(), any());
    }

    @Test
    void afterVerifyHookCalledOnSuccess() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xdef";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        boolean[] hookCalled = {false};
        filter.processor().onAfterVerify((payload, requirements, result) -> hookCalled[0] = true);

        filter.doFilter(req, resp, chain);

        assertTrue(hookCalled[0], "afterVerify hook should have been called");
    }

    @Test
    void beforeSettleHookCanAbort() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);
        when(resp.isCommitted()).thenReturn(false);

        filter.processor().onBeforeSettle((payload, requirements) ->
                PaymentHooks.AbortResult.abort("settle_blocked"));

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(fac, never()).settle(any(), any(), anyBoolean());
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    // -----------------------------------------------------------------------
    // Dynamic pricing test
    // -----------------------------------------------------------------------

    @Test
    void dynamicPricingResolvesFromRequest() throws Exception {
        PaymentProcessor.RouteConfig dynamicConfig = new PaymentProcessor.RouteConfig();
        dynamicConfig.network = "eip155:196";
        dynamicConfig.payTo = "0xReceiver";
        dynamicConfig.priceFunction = request -> "$1.00"; // dynamic: always $1

        PaymentFilter dynamicFilter = PaymentFilter.create(fac, Map.of(
                "GET /dynamic", dynamicConfig
        ));

        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/dynamic");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/dynamic"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        dynamicFilter.doFilter(req, resp, chain);

        // Should return 402 with amount resolved from dynamic function ($1.00 = 1000000)
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    // -----------------------------------------------------------------------
    // Settlement timeout tests
    // -----------------------------------------------------------------------

    @Test
    void settleTimeoutNoHookReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        pp.accepted = serverEchoedRequirements();
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xTimeoutTx";
        sr.network = "eip155:196";
        sr.status = "timeout";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        SettleResponse pending = new SettleResponse();
        pending.success = true;
        pending.status = "pending";
        when(fac.settleStatus("0xTimeoutTx")).thenReturn(pending);

        when(resp.getStatus()).thenReturn(200);
        when(resp.isCommitted()).thenReturn(false);

        // No hook — should return 402 after poll deadline
        filter.processor().pollDeadline(java.time.Duration.ofMillis(500))
              .pollInterval(java.time.Duration.ofMillis(200));

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void settleTimeoutHookExceptionTreatedAsNotConfirmed() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.payload = Map.of("signature", "0xabc");
        pp.accepted = new PaymentRequirements();
        pp.accepted.scheme = "exact";
        pp.accepted.network = "eip155:196";
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(pp.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xTimeoutTx";
        sr.network = "eip155:196";
        sr.status = "timeout";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        SettleResponse pending = new SettleResponse();
        pending.success = true;
        pending.status = "pending";
        when(fac.settleStatus("0xTimeoutTx")).thenReturn(pending);

        when(resp.getStatus()).thenReturn(200);

        filter.processor().pollDeadline(java.time.Duration.ofMillis(300))
              .pollInterval(java.time.Duration.ofMillis(100))
              .onSettlementTimeout((txHash, network) -> {
                  throw new RuntimeException("boom");
              });

        // Exception must not escape to caller; processor falls through to 402.
        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    // -----------------------------------------------------------------------
    // onProtectedRequest tests
    // -----------------------------------------------------------------------

    @Test
    void protectedRequestGrantAccessBypassesPayment() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        filter.processor().onProtectedRequest((request, routeConfig) ->
                PaymentHooks.ProtectedRequestResult.grantAccess());

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp, never()).setStatus(anyInt());
        verify(fac, never()).verify(any(), any());
        verify(fac, never()).settle(any(), any(), anyBoolean());
    }

    @Test
    void protectedRequestAbortReturns403WithReason() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        java.io.StringWriter body = new java.io.StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(body));

        filter.processor().onProtectedRequest((request, routeConfig) ->
                PaymentHooks.ProtectedRequestResult.abort("rate_limited"));

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(403);
        verify(chain, never()).doFilter(any(), any());
        verify(resp, never()).setHeader(eq("PAYMENT-REQUIRED"), any());
        verify(fac, never()).verify(any(), any());
        assertTrue(body.toString().contains("\"rate_limited\""),
                "403 body should include the abort reason; actual: " + body);
    }

    @Test
    void protectedRequestProceedFallsThroughToVerify() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("signature", "0xabc");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xdef";
        sr.network = "eip155:196";
        when(fac.settle(any(), any(), anyBoolean())).thenReturn(sr);

        filter.processor().onProtectedRequest((request, routeConfig) ->
                PaymentHooks.ProtectedRequestResult.proceed());

        filter.doFilter(req, resp, chain);

        verify(fac).verify(any(), any());
        verify(fac).settle(any(), any(), anyBoolean());
        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
    }

    @Test
    void protectedRequestFirstNonProceedWins() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        // Hook 1: proceed  → should fall through to hook 2.
        // Hook 2: grantAccess → wins.
        // Hook 3: abort → must NOT run because hook 2 already decided.
        boolean[] thirdRan = {false};
        filter.processor()
              .onProtectedRequest((r, c) -> PaymentHooks.ProtectedRequestResult.proceed())
              .onProtectedRequest((r, c) -> PaymentHooks.ProtectedRequestResult.grantAccess())
              .onProtectedRequest((r, c) -> {
                  thirdRan[0] = true;
                  return PaymentHooks.ProtectedRequestResult.abort("should not run");
              });

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(eq(req), any(jakarta.servlet.ServletResponse.class));
        verify(resp, never()).setStatus(anyInt());
        verify(fac, never()).verify(any(), any());
        assertFalse(thirdRan[0],
                "hook after a GRANT_ACCESS decision must not be invoked");
    }

    /**
     * Build a PaymentRequirements that echoes what the server would issue for
     * the {@code GET /protected/xlayer-usdt} route configured in {@link #init()}
     * — USDT on X Layer mainnet, {@code $0.01} → {@code "10000"} atomic units,
     * recipient {@code 0xReceiver}. The strict matcher in
     * {@link PaymentProcessor} requires the client to echo these fields, and
     * real clients (e.g. {@code OKXHttpClient}) do so by construction; this
     * helper keeps the timeout / settle / hook tests focused on their actual
     * concern instead of repeating the field setup.
     */
    private static PaymentRequirements serverEchoedRequirements() {
        PaymentRequirements r = new PaymentRequirements();
        r.scheme = "exact";
        r.network = "eip155:196";
        r.asset = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
        r.payTo = "0xReceiver";
        r.amount = "10000";
        return r;
    }
}
