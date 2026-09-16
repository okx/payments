package com.okx.payments.mpp.protocol;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

/** MPP method names. Phase 1 supports only "evm". */
public enum Method {
    EVM("evm");

    private final String wire;

    Method(String wire) {
        this.wire = wire;
    }

    @JsonValue
    public String wire() {
        return wire;
    }

    @JsonCreator
    public static Method fromWire(String s) {
        if (s == null) {
            throw new IllegalArgumentException("method must not be null");
        }
        for (Method m : values()) {
            if (m.wire.equalsIgnoreCase(s)) {
                return m;
            }
        }
        throw new IllegalArgumentException("Unsupported method: " + s);
    }
}
