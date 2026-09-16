package com.okx.payments.router;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.function.BiConsumer;

/**
 * PaymentRouter configuration.
 *
 * <p>Validation at construction:
 * <ul>
 *   <li>{@code routes} must be a {@code LinkedHashMap} (declaration order = match priority)</li>
 *   <li>Every adapter-name key inside any {@link RouteConfig} must reference a registered adapter</li>
 * </ul>
 */
public final class PaymentRouterConfig {

    private final LinkedHashMap<String, RouteConfig> routes;
    private final List<ProtocolAdapter> protocols;
    private final BiConsumer<Throwable, String> onError;

    public PaymentRouterConfig(LinkedHashMap<String, RouteConfig> routes,
                               List<ProtocolAdapter> protocols,
                               BiConsumer<Throwable, String> onError) {
        Objects.requireNonNull(routes, "routes");
        Objects.requireNonNull(protocols, "protocols");
        // routes is already typed as LinkedHashMap<String, RouteConfig> at this method's signature;
        // copy into a new LinkedHashMap for isolation.
        this.routes = new LinkedHashMap<>(routes);
        this.protocols = List.copyOf(protocols);
        this.onError = onError;
        validateAdapterReferences();
    }

    public LinkedHashMap<String, RouteConfig> routes() {
        return routes;
    }

    public List<ProtocolAdapter> protocols() {
        return protocols;
    }

    public BiConsumer<Throwable, String> onError() {
        return onError;
    }

    private void validateAdapterReferences() {
        java.util.Set<String> names = new java.util.HashSet<>();
        for (ProtocolAdapter a : protocols) {
            if (!names.add(a.name())) {
                throw new IllegalArgumentException("duplicate adapter name: " + a.name());
            }
        }
        for (Map.Entry<String, RouteConfig> e : routes.entrySet()) {
            for (String key : e.getValue().all().keySet()) {
                if (!names.contains(key)) {
                    throw new IllegalArgumentException(
                        "route '" + e.getKey() + "' references unknown adapter '" + key
                            + "'; registered: " + names);
                }
            }
        }
    }

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private LinkedHashMap<String, RouteConfig> routes = new LinkedHashMap<>();
        private final java.util.List<ProtocolAdapter> protocols = new java.util.ArrayList<>();
        private BiConsumer<Throwable, String> onError;

        public Builder route(String pattern, RouteConfig cfg) {
            routes.put(pattern, cfg);
            return this;
        }

        public Builder routes(LinkedHashMap<String, RouteConfig> v) {
            this.routes = new LinkedHashMap<>(v);
            return this;
        }

        public Builder protocol(ProtocolAdapter a) {
            protocols.add(a);
            return this;
        }

        public Builder onError(BiConsumer<Throwable, String> v) {
            this.onError = v;
            return this;
        }

        public PaymentRouterConfig build() {
            return new PaymentRouterConfig(routes, protocols, onError);
        }
    }

}
