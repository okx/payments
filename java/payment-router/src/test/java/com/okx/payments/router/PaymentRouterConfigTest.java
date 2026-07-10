package com.okx.payments.router;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class PaymentRouterConfigTest {

    @Test
    void rejects_route_referencing_unregistered_adapter() {
        LinkedHashMap<String, RouteConfig> routes = new LinkedHashMap<>();
        routes.put("/api/foo", RouteConfig.of().with("ghost", "x"));

        assertThatThrownBy(() -> new PaymentRouterConfig(routes,
            List.of(stubAdapter("mpp", 10)), null))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("ghost");
    }

    @Test
    void rejects_duplicate_adapter_names() {
        assertThatThrownBy(() -> new PaymentRouterConfig(new LinkedHashMap<>(),
            List.of(stubAdapter("mpp", 10), stubAdapter("mpp", 20)), null))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("duplicate");
    }

    @Test
    void preserves_route_declaration_order() {
        LinkedHashMap<String, RouteConfig> routes = new LinkedHashMap<>();
        routes.put("/a", RouteConfig.of());
        routes.put("/b", RouteConfig.of());
        routes.put("/c", RouteConfig.of());
        PaymentRouterConfig cfg = new PaymentRouterConfig(routes, List.of(), null);
        assertThat(cfg.routes().keySet()).containsExactly("/a", "/b", "/c");
    }

    @Test
    void builder_accumulates_routes_in_order() {
        PaymentRouterConfig cfg = PaymentRouterConfig.builder()
            .protocol(stubAdapter("mpp", 10))
            .protocol(stubAdapter("x402", 20))
            .route("/a", RouteConfig.of().with("mpp", "x"))
            .route("/b", RouteConfig.of().with("x402", "y"))
            .build();
        assertThat(cfg.routes().keySet()).containsExactly("/a", "/b");
        assertThat(cfg.protocols()).hasSize(2);
    }

    static ProtocolAdapter stubAdapter(String name, int prio) {
        return new ProtocolAdapter() {
            @Override public String name() { return name; }
            @Override public int priority() { return prio; }
            @Override public boolean detect(HttpServletRequest r) { return false; }
            @Override public CompletableFuture<Map<String, List<String>>> getChallenge(
                HttpServletRequest r, Object cfg) { return CompletableFuture.completedFuture(Map.of()); }
            @Override public void handle(
                HttpServletRequest r, HttpServletResponse w, javax.servlet.FilterChain chain, Object cfg) {
                /* noop */
            }
        };
    }
}
