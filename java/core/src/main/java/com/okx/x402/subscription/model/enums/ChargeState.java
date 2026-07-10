// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum ChargeState {
    PENDING(0),
    SUCCESS(1),
    FAILED(2);

    private final int value;

    ChargeState(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static ChargeState fromValue(int value) {
        for (ChargeState state : values()) {
            if (state.value == value) {
                return state;
            }
        }
        throw new IllegalArgumentException("Unknown ChargeState value: " + value);
    }
}
