// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum CancelInitiator {
    PAYER(0),
    MERCHANT(1);

    private final int value;

    CancelInitiator(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static CancelInitiator fromValue(int value) {
        for (CancelInitiator initiator : values()) {
            if (initiator.value == value) {
                return initiator;
            }
        }
        throw new IllegalArgumentException("Unknown CancelInitiator value: " + value);
    }
}
