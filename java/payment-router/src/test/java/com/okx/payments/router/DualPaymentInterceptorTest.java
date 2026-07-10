package com.okx.payments.router;

import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.x402.server.PaymentInterceptor;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Response;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.springframework.web.servlet.HandlerInterceptor;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.math.BigInteger;
import java.util.HashMap;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.doAnswer;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * Verifies the dispatch matrix in {@link DualPaymentInterceptor}:
 *  PAYMENT-SIGNATURE / X-PAYMENT      → x402 picked
 *  Authorization: Payment ...         → MPP picked
 *  X-Channel-Id                       → MPP picked
 *  none + URI in MPP routes           → MPP picked
 *  none + URI not in MPP routes       → x402 default
 *
 * And confirms that postHandle / afterCompletion route to the SAME delegate
 * that handled preHandle (no cross-talk).
 */
class DualPaymentInterceptorTest {

    private PaymentInterceptor    x402;
    private MppPaymentInterceptor mpp;
    private DualPaymentInterceptor dual;

    @BeforeEach
    void setup() {
        x402 = mock(PaymentInterceptor.class);
        mpp  = mock(MppPaymentInterceptor.class);
        dual = new DualPaymentInterceptor(x402, mpp);
    }

    private HttpServletRequest req(String uri) {
        HttpServletRequest r = mock(HttpServletRequest.class);
        when(r.getRequestURI()).thenReturn(uri);
        // Wire setAttribute/getAttribute round-trip — Mockito doesn't correlate by default.
        Map<String, Object> attrs = new HashMap<>();
        doAnswer(inv -> { attrs.put(inv.getArgument(0), inv.getArgument(1)); return null; })
            .when(r).setAttribute(anyString(), any());
        when(r.getAttribute(anyString())).thenAnswer(inv -> attrs.get(inv.getArgument(0)));
        return r;
    }

    // ── Dispatch matrix ───────────────────────────────────────────────────────

    @Test
    void payment_signature_header_picks_x402() {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        when(r.getHeader("PAYMENT-SIGNATURE")).thenReturn("abc");

        HandlerInterceptor picked = dual.pickDelegate(r);
        assertThat(picked).isSameAs(x402);
    }

    @Test
    void x_payment_header_picks_x402_v1_compat() {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        when(r.getHeader("X-PAYMENT")).thenReturn("abc");

        assertThat(dual.pickDelegate(r)).isSameAs(x402);
    }

    @Test
    void authorization_payment_header_picks_mpp() {
        HttpServletRequest r = req("/api/v6/dex/market/session/manage");
        when(r.getHeader("Authorization")).thenReturn("Payment eyJ…");

        assertThat(dual.pickDelegate(r)).isSameAs(mpp);
    }

    @Test
    void authorization_payment_lowercase_picks_mpp() {
        HttpServletRequest r = req("/api/v6/dex/market/session/manage");
        when(r.getHeader("Authorization")).thenReturn("payment eyJ…");

        assertThat(dual.pickDelegate(r)).isSameAs(mpp);
    }

    @Test
    void authorization_bearer_does_not_pick_mpp() {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        when(r.getHeader("Authorization")).thenReturn("Bearer xyz");
        when(mpp.matches(r)).thenReturn(false);

        // Bearer is for some other auth scheme → x402 default
        assertThat(dual.pickDelegate(r)).isSameAs(x402);
    }

    @Test
    void x_channel_id_header_picks_mpp() {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        when(r.getHeader("X-Channel-Id")).thenReturn("0xabc");

        assertThat(dual.pickDelegate(r)).isSameAs(mpp);
    }

    @Test
    void no_auth_uri_in_mpp_routes_picks_mpp() {
        HttpServletRequest r = req("/api/v6/dex/market/session/manage");
        when(mpp.matches(r)).thenReturn(true);

        assertThat(dual.pickDelegate(r)).isSameAs(mpp);
    }

    @Test
    void no_auth_uri_not_in_mpp_routes_picks_x402_default() {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        when(mpp.matches(r)).thenReturn(false);

        assertThat(dual.pickDelegate(r)).isSameAs(x402);
    }

    // ── Lifecycle routing ─────────────────────────────────────────────────────

    @Test
    void postHandle_routes_to_picked_delegate_x402() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(r.getHeader("PAYMENT-SIGNATURE")).thenReturn("abc");
        when(x402.preHandle(any(), any(), any())).thenReturn(true);

        dual.preHandle(r, resp, null);
        // attrs map (wired via req()) carries x402 from preHandle to postHandle

        dual.postHandle(r, resp, null, null);
        dual.afterCompletion(r, resp, null, null);

        verify(x402, times(1)).preHandle(any(), any(), any());
        verify(x402, times(1)).postHandle(any(), any(), any(), any());
        verify(x402, times(1)).afterCompletion(any(), any(), any(), any());

        verify(mpp, never()).preHandle(any(), any(), any());
        verify(mpp, never()).postHandle(any(), any(), any(), any());
        verify(mpp, never()).afterCompletion(any(), any(), any(), any());
    }

    @Test
    void postHandle_routes_to_picked_delegate_mpp() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(r.getHeader("X-Channel-Id")).thenReturn("0xabc");
        when(mpp.preHandle(any(), any(), any())).thenReturn(true);

        dual.preHandle(r, resp, null);
        // attrs map (wired via req()) carries mpp from preHandle to postHandle

        dual.postHandle(r, resp, null, null);
        dual.afterCompletion(r, resp, null, null);

        verify(mpp, times(1)).preHandle(any(), any(), any());
        verify(mpp, times(1)).postHandle(any(), any(), any(), any());
        verify(mpp, times(1)).afterCompletion(any(), any(), any(), any());

        verify(x402, never()).preHandle(any(), any(), any());
        verify(x402, never()).postHandle(any(), any(), any(), any());
        verify(x402, never()).afterCompletion(any(), any(), any(), any());
    }

    @Test
    void post_and_after_with_no_picked_attribute_are_noop() throws Exception {
        HttpServletRequest r = req("/some/path");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        // No preHandle ran → attribute map empty → getAttribute returns null

        dual.postHandle(r, resp, null, null);
        dual.afterCompletion(r, resp, null, null);

        verify(x402, never()).postHandle(any(), any(), any(), any());
        verify(mpp,  never()).postHandle(any(), any(), any(), any());
        verify(x402, never()).afterCompletion(any(), any(), any(), any());
        verify(mpp,  never()).afterCompletion(any(), any(), any(), any());
    }

    @Test
    void preHandle_returns_what_picked_delegate_returns() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(r.getHeader("PAYMENT-SIGNATURE")).thenReturn("abc");
        when(x402.preHandle(any(), any(), any())).thenReturn(false);

        boolean cont = dual.preHandle(r, resp, null);
        assertThat(cont).isFalse();
    }

    @Test
    void preHandle_stores_picked_attr_for_routing() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/session/manage");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(mpp.matches(r)).thenReturn(true);
        when(mpp.preHandle(any(), any(), any())).thenReturn(false);

        dual.preHandle(r, resp, null);

        verify(r).setAttribute(eq(DualPaymentInterceptor.ATTR_PICKED_DELEGATE), eq(mpp));
    }

    // ── First-touch challenge merging (both protocols cover the URL) ─────────

    /**
     * No payment header + URL covered by both MPP and x402 → preHandle writes
     * a merged 402 envelope (WWW-Authenticate + PAYMENT-REQUIRED) and returns
     * false. Neither delegate's preHandle is invoked.
     */
    @Test
    void first_touch_with_both_protocols_emits_merged_402() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter body = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(body));
        when(resp.isCommitted()).thenReturn(false);

        // MPP route table covers the URL → tryBuildMppChallenges returns non-empty
        MppRouteConfig routes = mock(MppRouteConfig.class);
        MppRouteConfig.Entry entry = mock(MppRouteConfig.Entry.class);
        when(entry.realm()).thenReturn("dex.market");
        when(entry.options()).thenReturn(java.util.List.of(mock(MppRouteConfig.Option.class)));
        when(entry.buildSessionRequests()).thenReturn(java.util.List.of(
            new com.okx.payments.mpp.protocol.session.SessionRequest(
                "1", "0xUSDC", "0xPayee", "request", null, null, null, null)));
        when(routes.match("/api/v6/dex/market/price")).thenReturn(entry);
        when(mpp.routes()).thenReturn(routes);

        MppServer server = mock(MppServer.class);
        Challenge ch = new Challenge("id-1", "dex.market", Method.EVM, Intent.SESSION,
            "request-b64", "2026-01-01T00:00:00Z", null, null, null);
        when(server.request(eq("dex.market"), eq(Intent.SESSION), any())).thenReturn(ch);
        when(mpp.mppServer()).thenReturn(server);

        // x402 processor — wire a real PaymentProcessor whose preHandle writes
        // PAYMENT-REQUIRED via respond402. Simpler: mock the processor to
        // emulate the side effect on the capturing response.
        PaymentProcessor processor = mock(PaymentProcessor.class);
        when(x402.processor()).thenReturn(processor);
        doAnswer(inv -> {
            X402Response captured = inv.getArgument(1);
            captured.setHeader("PAYMENT-REQUIRED", "eyJ4NDAyVmVyc2lvbiI6Mn0");
            captured.setStatus(402);
            return null;
        }).when(processor).preHandle(any(), any());

        boolean cont = dual.preHandle(r, resp, null);

        assertThat(cont).isFalse();
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(resp).addHeader(eq("WWW-Authenticate"), anyString());
        verify(resp).setHeader(eq("PAYMENT-REQUIRED"), eq("eyJ4NDAyVmVyc2lvbiI6Mn0"));
        verify(resp).setContentType("application/problem+json");

        // Neither delegate's preHandle was invoked — the dual interceptor
        // owned the response.
        verify(x402, never()).preHandle(any(), any(), any());
        verify(mpp,  never()).preHandle(any(), any(), any());
    }

    /**
     * When only MPP covers the URL, the merge fast-path skips and the legacy
     * pickDelegate matrix runs — MPP wins because its route table covers.
     */
    @Test
    void first_touch_with_only_mpp_covered_falls_back_to_pick() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/session/manage");
        HttpServletResponse resp = mock(HttpServletResponse.class);

        // MPP covers; x402 processor returns no PAYMENT-REQUIRED (no route).
        MppRouteConfig routes = mock(MppRouteConfig.class);
        MppRouteConfig.Entry entry = mock(MppRouteConfig.Entry.class);
        when(entry.realm()).thenReturn("dex.market");
        when(entry.options()).thenReturn(java.util.List.of(mock(MppRouteConfig.Option.class)));
        when(entry.buildSessionRequests()).thenReturn(java.util.List.of(
            new com.okx.payments.mpp.protocol.session.SessionRequest(
                "1", "0xUSDC", "0xPayee", "request", null, null, null, null)));
        when(routes.match("/api/v6/dex/market/session/manage")).thenReturn(entry);
        when(mpp.routes()).thenReturn(routes);

        MppServer server = mock(MppServer.class);
        when(server.request(anyString(), any(), any())).thenReturn(
            new Challenge("id", "dex.market", Method.EVM, Intent.SESSION,
                "req", "2026-01-01T00:00:00Z", null, null, null));
        when(mpp.mppServer()).thenReturn(server);

        PaymentProcessor processor = mock(PaymentProcessor.class);
        when(x402.processor()).thenReturn(processor);
        // Processor preHandle writes nothing → no PAYMENT-REQUIRED captured.

        when(mpp.matches(r)).thenReturn(true);
        when(mpp.preHandle(any(), any(), any())).thenReturn(false);

        dual.preHandle(r, resp, null);

        verify(resp, never()).addHeader(eq("WWW-Authenticate"), anyString());
        verify(mpp, times(1)).preHandle(any(), any(), any());
    }

    /**
     * Multi-token MPP route: merge fast-path emits one WWW-Authenticate line
     * per option (USDC, USDT, OKB → 3 lines) plus one PAYMENT-REQUIRED for x402.
     */
    @Test
    void first_touch_multi_token_mpp_emits_one_wwwauth_per_option() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter body = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(body));
        when(resp.isCommitted()).thenReturn(false);

        MppRouteConfig routes = mock(MppRouteConfig.class);
        MppRouteConfig.Entry entry = mock(MppRouteConfig.Entry.class);
        when(entry.realm()).thenReturn("dex.market");
        // 3 mock options → entry emits 3 SessionRequests
        when(entry.options()).thenReturn(java.util.List.of(
            mock(MppRouteConfig.Option.class),
            mock(MppRouteConfig.Option.class),
            mock(MppRouteConfig.Option.class)));
        when(entry.buildSessionRequests()).thenReturn(java.util.List.of(
            new com.okx.payments.mpp.protocol.session.SessionRequest(
                "1",     "0xUSDC", "0xPayee", "request", null, null, null, null),
            new com.okx.payments.mpp.protocol.session.SessionRequest(
                "2",     "0xUSDT", "0xPayee", "request", null, null, null, null),
            new com.okx.payments.mpp.protocol.session.SessionRequest(
                "50000", "0xOKB",  "0xPayee", "request", null, null, null, null)));
        when(routes.match("/api/v6/dex/market/price")).thenReturn(entry);
        when(mpp.routes()).thenReturn(routes);

        MppServer server = mock(MppServer.class);
        when(server.request(eq("dex.market"), eq(Intent.SESSION), any())).thenAnswer(inv -> {
            com.okx.payments.mpp.protocol.session.SessionRequest sr = inv.getArgument(2);
            // distinct id per option so we can confirm 3 separate emits below
            return new Challenge("id-" + sr.currency(), "dex.market", Method.EVM,
                Intent.SESSION, "request-b64", "2026-01-01T00:00:00Z", null, null, null);
        });
        when(mpp.mppServer()).thenReturn(server);

        PaymentProcessor processor = mock(PaymentProcessor.class);
        when(x402.processor()).thenReturn(processor);
        doAnswer(inv -> {
            X402Response captured = inv.getArgument(1);
            captured.setHeader("PAYMENT-REQUIRED", "eyJ4NDAyVmVyc2lvbiI6Mn0");
            captured.setStatus(402);
            return null;
        }).when(processor).preHandle(any(), any());

        dual.preHandle(r, resp, null);

        // 3 WWW-Authenticate lines (one per token option) + 1 PAYMENT-REQUIRED.
        verify(resp, times(3)).addHeader(eq("WWW-Authenticate"), anyString());
        verify(resp).setHeader(eq("PAYMENT-REQUIRED"), eq("eyJ4NDAyVmVyc2lvbiI6Mn0"));
    }

    /**
     * Disabling merge via {@link DualPaymentInterceptor#mergeFirstTouchChallenges(boolean)}
     * restores legacy single-delegate behavior even when both protocols cover the URL.
     */
    @Test
    void merge_opt_out_falls_back_to_x402_default() throws Exception {
        HttpServletRequest r = req("/api/v6/dex/market/price");
        HttpServletResponse resp = mock(HttpServletResponse.class);

        // Both would cover, but merge is disabled.
        MppRouteConfig routes = mock(MppRouteConfig.class);
        MppRouteConfig.Entry entry = mock(MppRouteConfig.Entry.class);
        when(routes.match("/api/v6/dex/market/price")).thenReturn(entry);
        when(mpp.routes()).thenReturn(routes);
        when(mpp.matches(r)).thenReturn(false);  // not in first-touch route table

        when(x402.preHandle(any(), any(), any())).thenReturn(true);

        dual.mergeFirstTouchChallenges(false);
        dual.preHandle(r, resp, null);

        verify(x402, times(1)).preHandle(any(), any(), any());
        verify(mpp,  never()).preHandle(any(), any(), any());
    }
}
