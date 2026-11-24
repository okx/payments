// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum PendingChangeState {
    PENDING(0),
    ACTIVATED(1),
    CANCELED(2),
    EXPIRED(3);

    private final int value;

    PendingChangeState(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static PendingChangeState fromValue(int value) {
        for (PendingChangeState state : values()) {
            if (state.value == value) {
                return state;
            }
        }
        throw new IllegalArgumentException("Unknown PendingChangeState value: " + value);
    }
}
