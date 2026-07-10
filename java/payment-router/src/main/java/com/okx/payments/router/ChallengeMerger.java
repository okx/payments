package com.okx.payments.router;

import javax.servlet.http.HttpServletRequest;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.function.BiConsumer;

/**
 * Concurrent challenge collector — invokes every adapter's {@link ProtocolAdapter#getChallenge}
 * in parallel, merges results into a {@code Map<headerName, List<value>>} preserving multi-value
 * semantics so each value can be emitted as its own header line (D-UPM P0-1).
 *
 * <p>An adapter that throws or returns {@code null}/empty is silently skipped (and reported via
 * {@code onError} when supplied).
 */
public final class ChallengeMerger {

    private final List<ProtocolAdapter> adapters;
    private final BiConsumer<Throwable, String> onError;

    public ChallengeMerger(List<ProtocolAdapter> adapters, BiConsumer<Throwable, String> onError) {
        this.adapters = List.copyOf(adapters);
        this.onError = onError;
    }

    public Map<String, List<String>> merge(HttpServletRequest req, RouteConfig routeConfig) {
        List<CompletableFuture<Map<String, List<String>>>> futures = new ArrayList<>(adapters.size());
        for (ProtocolAdapter a : adapters) {
            Object cfg = routeConfig == null ? null : routeConfig.get(a.name());
            CompletableFuture<Map<String, List<String>>> f;
            try {
                f = a.getChallenge(req, cfg);
            } catch (RuntimeException e) {
                if (onError != null) onError.accept(e, a.name());
                continue;
            }
            if (f == null) continue;
            futures.add(f.exceptionally(ex -> {
                if (onError != null) onError.accept(ex, a.name());
                return null;
            }));
        }

        Map<String, List<String>> merged = new LinkedHashMap<>();
        for (CompletableFuture<Map<String, List<String>>> f : futures) {
            Map<String, List<String>> per;
            try {
                per = f.join();
            } catch (RuntimeException e) {
                continue;
            }
            if (per == null || per.isEmpty()) continue;
            for (Map.Entry<String, List<String>> e : per.entrySet()) {
                merged.computeIfAbsent(e.getKey(), k -> new ArrayList<>()).addAll(e.getValue());
            }
        }
        return merged;
    }
}
