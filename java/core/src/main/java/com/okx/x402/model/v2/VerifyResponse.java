// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.Map;

/** Response from facilitator /verify endpoint (V2). */
public class VerifyResponse {
    /** Whether the payment is valid. */
    public boolean isValid;
    /** Machine-readable reason for invalidity. */
    public String invalidReason;
    /** Human-readable message for invalidity (V2 new). */
    public String invalidMessage;
    /** Payer wallet address. */
    public String payer;
    /** Optional extensions. */
    public Map<String, Object> extensions;
}
