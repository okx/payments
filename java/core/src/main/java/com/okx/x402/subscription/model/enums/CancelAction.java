// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum CancelAction {
    CANCEL_SUBSCRIPTION(0),
    CANCEL_PENDING_CHANGE(1);

    private final int value;

    CancelAction(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static CancelAction fromValue(int value) {
        for (CancelAction action : values()) {
            if (action.value == value) {
                return action;
            }
        }
        throw new IllegalArgumentException("Unknown CancelAction value: " + value);
    }
}
