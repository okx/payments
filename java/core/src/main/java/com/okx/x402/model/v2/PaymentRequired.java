// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** HTTP 402 response body (V2 protocol). */
public class PaymentRequired {
    /** Protocol version. */
    public int x402Version = 2;
    /** Error description if applicable. */
    public String error;
    /** Resource information. */
    public ResourceInfo resource;
    /** List of acceptable payment methods. */
    public List<PaymentRequirements> accepts = new ArrayList<>();
    /** Optional extensions. */
    public Map<String, Object> extensions;
}
