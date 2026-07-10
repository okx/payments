package com.okx.payments.router;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.Test;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicReference;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.Mockito.mock;

class ChallengeMergerTest {

    @Test
    void merges_multi_value_headers_preserving_each_value() {
        ProtocolAdapter mpp = adapter("mpp", 10, () -> Map.of("WWW-Authenticate", List.of("Payment id=A")));
        ProtocolAdapter x402 = adapter("x402", 20, () -> Map.of(
            "WWW-Authenticate", List.of("X402 v=2"),
            "PAYMENT-REQUIRED", List.of("{\"x402Version\":2}")));

        Map<String, List<String>> merged = new ChallengeMerger(List.of(mpp, x402), null)
            .merge(mock(HttpServletRequest.class), RouteConfig.of().with("mpp", "x").with("x402", "y"));

        assertThat(merged.get("WWW-Authenticate")).containsExactlyInAnyOrder("Payment id=A", "X402 v=2");
        assertThat(merged.get("PAYMENT-REQUIRED")).containsExactly("{\"x402Version\":2}");
    }

    @Test
    void single_failure_is_isolated_and_reported_to_on_error() {
        AtomicReference<String> capturedAdapter = new AtomicReference<>();
        ProtocolAdapter ok = adapter("ok", 10, () -> Map.of("WWW-Authenticate", List.of("OK")));
        ProtocolAdapter boom = boomAdapter("boom", 20);

        Map<String, List<String>> merged = new ChallengeMerger(List.of(ok, boom),
            (err, name) -> capturedAdapter.set(name))
            .merge(mock(HttpServletRequest.class), RouteConfig.of());

        assertThat(merged.get("WWW-Authenticate")).containsExactly("OK");
        assertThat(capturedAdapter.get()).isEqualTo("boom");
    }

    @Test
    void null_or_empty_response_skipped() {
        ProtocolAdapter empty = adapter("a", 10, () -> Map.of());
        ProtocolAdapter nullCh = new ProtocolAdapter() {
            @Override public String name() { return "b"; }
            @Override public int priority() { return 20; }
            @Override public boolean detect(HttpServletRequest r) { return false; }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.completedFuture(null);
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w, javax.servlet.FilterChain chain, Object cfg) {
                /* noop */
            }
        };
        Map<String, List<String>> merged = new ChallengeMerger(List.of(empty, nullCh), null)
            .merge(mock(HttpServletRequest.class), RouteConfig.of());
        assertThat(merged).isEmpty();
    }

    private static ProtocolAdapter adapter(String name, int prio, java.util.function.Supplier<Map<String, List<String>>> headers) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) { return false; }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.completedFuture(headers.get());
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w, javax.servlet.FilterChain chain, Object cfg) {
                /* noop */
            }
        };
    }

    private static ProtocolAdapter boomAdapter(String name, int prio) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) { return false; }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.failedFuture(new IllegalStateException("boom"));
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w, javax.servlet.FilterChain chain, Object cfg) {
                /* noop */
            }
        };
    }
}
