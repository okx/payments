package com.okx.payments.router;

import javax.servlet.FilterChain;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;

import java.io.IOException;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class PaymentRouterFilterTest {

    @Test
    void unmatched_route_passes_through_chain() throws Exception {
        PaymentRouterFilter f = new PaymentRouterFilter(PaymentRouterConfig.builder()
            .protocol(stub("mpp", 10))
            .route("/api/foo", RouteConfig.of().with("mpp", "x"))
            .build());

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/some/other/path");
        FilterChain chain = mock(FilterChain.class);

        f.doFilter(req, resp, chain);
        verify(chain, times(1)).doFilter(req, resp);
    }

    @Test
    void detected_protocol_handler_invoked() throws Exception {
        ProtocolAdapter mpp = stubDetecting("mpp", 10);
        PaymentRouterFilter f = new PaymentRouterFilter(PaymentRouterConfig.builder()
            .protocol(mpp)
            .route("/api/weather", RouteConfig.of().with("mpp", "x"))
            .build());

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/weather");
        when(req.getHeader("Authorization")).thenReturn("Payment xyz");
        FilterChain chain = mock(FilterChain.class);

        f.doFilter(req, resp, chain);
        // handler must have set the marker header
        verify(resp, times(1)).setHeader(eq("X-Mpp-Detected"), eq("true"));
        verify(chain, times(0)).doFilter(any(), any());
    }

    @Test
    void no_detection_emits_402_with_multi_line_headers() throws Exception {
        ProtocolAdapter mpp = stubChallenger("mpp", 10,
            Map.of("WWW-Authenticate", List.of("Payment id=A, realm=demo")));
        ProtocolAdapter x402 = stubChallenger("x402", 20,
            Map.of("WWW-Authenticate", List.of("X402 v=2"),
                   "PAYMENT-REQUIRED", List.of("{\"x402Version\":2}")));

        PaymentRouterFilter f = new PaymentRouterFilter(PaymentRouterConfig.builder()
            .protocol(mpp).protocol(x402)
            .route("/api/weather", RouteConfig.of().with("mpp", "x").with("x402", "y"))
            .build());

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/api/weather");
        when(req.getHeader("Authorization")).thenReturn(null);
        StringWriter sw = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));

        f.doFilter(req, resp, mock(FilterChain.class));

        verify(resp).setStatus(402);

        // Capture all addHeader calls — assert WWW-Authenticate appeared TWICE (multi-line, NOT joined).
        ArgumentCaptor<String> nameCap = ArgumentCaptor.forClass(String.class);
        ArgumentCaptor<String> valCap = ArgumentCaptor.forClass(String.class);
        verify(resp, times(3)).addHeader(nameCap.capture(), valCap.capture());

        // Build a multimap of header → values
        Map<String, List<String>> emitted = new LinkedHashMap<>();
        for (int i = 0; i < nameCap.getAllValues().size(); i++) {
            emitted.computeIfAbsent(nameCap.getAllValues().get(i), k -> new ArrayList<>())
                .add(valCap.getAllValues().get(i));
        }

        assertThat(emitted.get("WWW-Authenticate"))
            .as("WWW-Authenticate must be emitted once per challenge value, not joined")
            .containsExactlyInAnyOrder("Payment id=A, realm=demo", "X402 v=2");
        assertThat(emitted.get("PAYMENT-REQUIRED")).containsExactly("{\"x402Version\":2}");
        assertThat(sw.toString()).contains("\"status\":402");
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    private static ProtocolAdapter stub(String name, int prio) {
        return stubChallenger(name, prio, Map.of());
    }

    private static ProtocolAdapter stubDetecting(String name, int prio) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) {
                String h = r.getHeader("Authorization");
                return h != null && h.startsWith("Payment ");
            }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.completedFuture(Map.of());
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w,
                                         FilterChain chain, Object cfg) {
                w.setHeader("X-Mpp-Detected", "true");
            }
        };
    }

    private static ProtocolAdapter stubChallenger(String name, int prio, Map<String, List<String>> headers) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) { return false; }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.completedFuture(headers);
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w,
                                         FilterChain chain, Object cfg) {
                /* noop */
            }
        };
    }

    @SuppressWarnings("unused")
    private static void unused() throws IOException {
    }
}
