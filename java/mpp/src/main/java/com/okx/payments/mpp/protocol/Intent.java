package com.okx.payments.mpp.protocol;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum Intent {
    CHARGE("charge"),
    SESSION("session");

    private final String wire;

    Intent(String wire) {
        this.wire = wire;
    }

    @JsonValue
    public String wire() {
        return wire;
    }

    @JsonCreator
    public static Intent fromWire(String s) {
        if (s == null) {
            throw new IllegalArgumentException("intent must not be null");
        }
        for (Intent i : values()) {
            if (i.wire.equalsIgnoreCase(s)) {
                return i;
            }
        }
        throw new IllegalArgumentException("Unsupported intent: " + s);
    }
}
