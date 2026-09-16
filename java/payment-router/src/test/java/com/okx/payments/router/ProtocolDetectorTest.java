package com.okx.payments.router;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicInteger;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.Mockito.mock;

class ProtocolDetectorTest {

    @Test
    void calls_in_priority_order_short_circuits_on_match() {
        AtomicInteger calls = new AtomicInteger();
        ProtocolDetector det = new ProtocolDetector(List.of(
            adapter("low", 30, r -> { calls.incrementAndGet(); return false; }),
            adapter("mid", 20, r -> { calls.incrementAndGet(); return true; }),  // matches
            adapter("high", 10, r -> { calls.incrementAndGet(); return false; })
        ));

        ProtocolAdapter matched = det.detect(mock(HttpServletRequest.class));
        assertThat(matched.name()).isEqualTo("mid");
        // Priority order: high (10) first, then mid (20) — short-circuits there → 2 invocations.
        assertThat(calls.get()).isEqualTo(2);
    }

    @Test
    void no_match_returns_null() {
        ProtocolDetector det = new ProtocolDetector(List.of(
            adapter("a", 10, r -> false),
            adapter("b", 20, r -> false)));
        assertThat(det.detect(mock(HttpServletRequest.class))).isNull();
    }

    @Test
    void exception_in_detect_treated_as_no_match() {
        ProtocolDetector det = new ProtocolDetector(List.of(
            adapter("crash", 10, r -> { throw new RuntimeException("boom"); }),
            adapter("ok", 20, r -> true)));
        ProtocolAdapter matched = det.detect(mock(HttpServletRequest.class));
        assertThat(matched.name()).isEqualTo("ok");
    }

    private static ProtocolAdapter adapter(String name, int prio, java.util.function.Predicate<HttpServletRequest> det) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) { return det.test(r); }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest r, Object cfg) {
                return CompletableFuture.completedFuture(Map.of());
            }
            @Override public void handle(HttpServletRequest r, HttpServletResponse w, javax.servlet.FilterChain chain, Object cfg) {
                /* noop */
            }
        };
    }
}
