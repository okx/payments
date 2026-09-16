package com.okx.payments.router;

import javax.servlet.http.HttpServletRequest;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/** Serial short-circuit detect — D-UPM §10.10 #1. */
public final class ProtocolDetector {

    private final List<ProtocolAdapter> sortedAdapters;

    public ProtocolDetector(List<ProtocolAdapter> adapters) {
        List<ProtocolAdapter> copy = new ArrayList<>(adapters);
        copy.sort(Comparator.comparingInt(ProtocolAdapter::priority));
        this.sortedAdapters = List.copyOf(copy);
    }

    /** Returns the first adapter whose {@link ProtocolAdapter#detect} returns true. */
    public ProtocolAdapter detect(HttpServletRequest request) {
        for (ProtocolAdapter a : sortedAdapters) {
            try {
                if (a.detect(request)) {
                    return a;
                }
            } catch (RuntimeException ignored) {
                // Swallow per spec — detect must never raise 500.
            }
        }
        return null;
    }
}
