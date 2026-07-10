// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.enums;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonValue;

public enum ChargeType {
    INITIAL(1),
    PERIODIC(2),
    DOWNGRADE_FIRST_PERIOD(3),
    FINALIZE_EXPIRED_MARKER(4);

    private final int value;

    ChargeType(int value) {
        this.value = value;
    }

    @JsonValue
    public int getValue() {
        return value;
    }

    @JsonCreator
    public static ChargeType fromValue(int value) {
        for (ChargeType type : values()) {
            if (type.value == value) {
                return type;
            }
        }
        throw new IllegalArgumentException("Unknown ChargeType value: " + value);
    }
}
