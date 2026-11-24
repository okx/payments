package com.okx.payments.router;

import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Per-route configuration: a map keyed by adapter name (e.g. {@code "mpp"}, {@code "x402"})
 * with adapter-specific config payloads as values.
 */
public final class RouteConfig {

    private final Map<String, Object> adapterConfigs;

    public RouteConfig(Map<String, Object> adapterConfigs) {
        this.adapterConfigs = new LinkedHashMap<>(adapterConfigs);
    }

    public static RouteConfig of() {
        return new RouteConfig(new LinkedHashMap<>());
    }

    public RouteConfig with(String adapterName, Object cfg) {
        adapterConfigs.put(adapterName, cfg);
        return this;
    }

    public Object get(String adapterName) {
        return adapterConfigs.get(adapterName);
    }

    public Map<String, Object> all() {
        return adapterConfigs;
    }
}
