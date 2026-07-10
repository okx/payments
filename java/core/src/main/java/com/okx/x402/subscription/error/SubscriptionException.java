// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.error;

import java.io.IOException;

public class SubscriptionException extends IOException {

    private final String code;
    private final SubscriptionErrorCodes.Category category;

    public SubscriptionException(String code, String message) {
        super(message);
        // Doc-§6 alias spellings normalize to the canonical SA-implementation code so that
        // getCode() string comparisons hold whichever variant the backend emitted.
        this.code = SubscriptionErrorCodes.canonicalize(code);
        this.category = SubscriptionErrorCodes.classify(code);
    }

    public SubscriptionException(String code, String message, Throwable cause) {
        super(message, cause);
        this.code = SubscriptionErrorCodes.canonicalize(code);
        this.category = SubscriptionErrorCodes.classify(code);
    }

    public String getCode() {
        return code;
    }

    public SubscriptionErrorCodes.Category getCategory() {
        return category;
    }

    public boolean isRetryable() {
        return category == SubscriptionErrorCodes.Category.RETRYABLE;
    }

    public boolean isSelfHeal() {
        return category == SubscriptionErrorCodes.Category.SELF_HEAL;
    }
}
