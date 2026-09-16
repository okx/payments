// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum ChangeEffectiveAt {
    NONE(0),
    IMMEDIATE(1),
    PERIOD_END(2);

    private final int value;

    ChangeEffectiveAt(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static ChangeEffectiveAt fromValue(int value) {
        for (ChangeEffectiveAt effectiveAt : values()) {
            if (effectiveAt.value == value) {
                return effectiveAt;
            }
        }
        throw new IllegalArgumentException("Unknown ChangeEffectiveAt value: " + value);
    }
}
