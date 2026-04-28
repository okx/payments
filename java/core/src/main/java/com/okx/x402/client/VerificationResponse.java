// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

/**
 * JSON returned by POST /verify on a V1 facilitator.
 */
public class VerificationResponse {
    /** Whether the payment verification succeeded. */
    public boolean isValid;

    /** Reason for verification failure. */
    public String invalidReason;
}
