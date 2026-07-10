package com.okx.payments.router.adapters;

import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.seller.MppServer;
import javax.servlet.FilterChain;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class MppAdapterTest {

    private final MppServer mppServer = mock(MppServer.class);
    private final MppAdapter adapter = new MppAdapter(mppServer);

    @Test
    void detect_matches_authorization_payment_header_case_insensitive() {
        HttpServletRequest req = mock(HttpServletRequest.class);
        when(req.getHeader("Authorization")).thenReturn("Payment eyJ...");
        assertThat(adapter.detect(req)).isTrue();

        when(req.getHeader("Authorization")).thenReturn("payment eyJ...");
        assertThat(adapter.detect(req)).isTrue();
    }

    @Test
    void detect_skips_other_auth_schemes() {
        HttpServletRequest req = mock(HttpServletRequest.class);
        when(req.getHeader("Authorization")).thenReturn("Bearer xyz");
        assertThat(adapter.detect(req)).isFalse();

        when(req.getHeader("Authorization")).thenReturn(null);
        assertThat(adapter.detect(req)).isFalse();
    }

    @Test
    void name_and_priority_defaults() {
        assertThat(adapter.name()).isEqualTo("mpp");
        assertThat(adapter.priority()).isEqualTo(10);
    }

    @Test
    void get_challenge_serializes_www_authenticate_header() {
        Challenge ch = new Challenge("id-abc", "demo.example", Method.EVM, Intent.CHARGE,
            "eyJjcmVk", "2026-04-01T12:05:00Z", null, null, null);
        when(mppServer.request(eq("demo.example"), eq(Intent.CHARGE), any())).thenReturn(ch);

        Map<String, Object> route = Map.of(
            "realm", "demo.example",
            "intent", "charge",
            "requestPayload", Map.of("amount", "10000"));
        Map<String, List<String>> headers = adapter.getChallenge(
            mock(HttpServletRequest.class), route).join();

        assertThat(headers).containsKey("WWW-Authenticate");
        String value = headers.get("WWW-Authenticate").get(0);
        assertThat(value).startsWith("Payment ");
        assertThat(value).contains("id=\"id-abc\"");
        assertThat(value).contains("realm=\"demo.example\"");
        assertThat(value).contains("method=\"evm\"");
        assertThat(value).contains("intent=\"charge\"");
    }

    @Test
    void get_challenge_returns_empty_when_route_cfg_null() {
        Map<String, List<String>> headers = adapter.getChallenge(
            mock(HttpServletRequest.class), null).join();
        assertThat(headers).isEmpty();
    }

    @Test
    void serialize_www_auth_static_helper_handles_optional_fields() {
        Challenge ch = new Challenge("x", "r", Method.EVM, Intent.SESSION,
            "REQ", "EXPIRES", "Test", null, null);
        String s = MppAdapter.serializeWwwAuth(ch);
        assertThat(s).startsWith("Payment ");
        assertThat(s).contains("description=\"Test\"");
    }

    @Test
    void handle_legacy_cfg_passes_through_chain_with_marker() throws Exception {
        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        FilterChain chain = mock(FilterChain.class);

        // Legacy cfg path: not an MppRouteConfig.Entry → backward-compat marker + chain.doFilter
        adapter.handle(req, resp, chain, null);

        verify(resp).setHeader("X-Mpp-Detected", "true");
        verify(chain, times(1)).doFilter(req, resp);
    }
}
