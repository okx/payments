package com.okx.payments.router;

import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;

import static org.assertj.core.api.Assertions.assertThat;

class RouteMatcherTest {

    @Test
    void exact_match_with_method() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("GET /api/weather", RouteConfig.of().with("mpp", "x"));
        RouteMatcher m = new RouteMatcher(r);
        assertThat(m.match("GET", "/api/weather")).isNotNull();
        assertThat(m.match("POST", "/api/weather")).isNull();
    }

    @Test
    void match_strips_query_string() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        assertThat(new RouteMatcher(r).match("GET", "/api/foo?q=1&t=2")).isNotNull();
    }

    @Test
    void match_strips_fragment() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        assertThat(new RouteMatcher(r).match("GET", "/api/foo#section")).isNotNull();
    }

    @Test
    void match_collapses_double_slashes() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        assertThat(new RouteMatcher(r).match("GET", "//api//foo")).isNotNull();
    }

    @Test
    void match_strips_trailing_slash() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        assertThat(new RouteMatcher(r).match("GET", "/api/foo/")).isNotNull();
    }

    @Test
    void any_method_when_no_method_prefix() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        RouteMatcher m = new RouteMatcher(r);
        assertThat(m.match("GET", "/api/foo")).isNotNull();
        assertThat(m.match("POST", "/api/foo")).isNotNull();
        assertThat(m.match("DELETE", "/api/foo")).isNotNull();
    }

    @Test
    void declaration_order_first_match_wins() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/specific", RouteConfig.of().with("mpp", "specific"));
        r.put("/api/*", RouteConfig.of().with("mpp", "wild"));
        RouteMatcher m = new RouteMatcher(r);
        assertThat(m.match("GET", "/api/specific").config().get("mpp")).isEqualTo("specific");
        assertThat(m.match("GET", "/api/other").config().get("mpp")).isEqualTo("wild");
    }

    @Test
    void colon_param_matches_any_segment() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/users/:id", RouteConfig.of());
        RouteMatcher m = new RouteMatcher(r);
        assertThat(m.match("GET", "/api/users/123")).isNotNull();
        assertThat(m.match("GET", "/api/users/abc")).isNotNull();
        assertThat(m.match("GET", "/api/users/123/extra")).isNull();
    }

    @Test
    void wildcard_matches_descendants() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/*", RouteConfig.of());
        RouteMatcher m = new RouteMatcher(r);
        // After path normalization "/api/" → "/api", which does NOT match "/api/.*?".
        // Use explicit children to assert wildcard semantics.
        assertThat(m.match("GET", "/api/anything")).isNotNull();
        assertThat(m.match("GET", "/api/anything/here")).isNotNull();
        assertThat(m.match("GET", "/api")).isNull();   // bare path doesn't match /api/*
    }

    @Test
    void no_match_returns_null() {
        LinkedHashMap<String, RouteConfig> r = new LinkedHashMap<>();
        r.put("/api/foo", RouteConfig.of());
        assertThat(new RouteMatcher(r).match("GET", "/other/path")).isNull();
    }
}
