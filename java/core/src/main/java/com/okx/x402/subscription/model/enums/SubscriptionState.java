// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum SubscriptionState {
    PENDING(0),
    ACTIVE(1),
    COMPLETED(2),
    CANCELED(3),
    CHANGED(4),
    FAILED(99);

    private final int value;

    SubscriptionState(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static SubscriptionState fromValue(int value) {
        for (SubscriptionState state : values()) {
            if (state.value == value) {
                return state;
            }
        }
        throw new IllegalArgumentException("Unknown SubscriptionState value: " + value);
    }
}
