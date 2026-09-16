// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v1;

import java.util.ArrayList;
import java.util.List;

/**
 * The 402 Payment Required response body (v1).
 */
public class PaymentRequiredResponse {

    /** The x402 protocol version. */
    public int x402Version;

    /** The list of accepted payment requirements. */
    public List<PaymentRequirements> accepts = new ArrayList<>();

    /** An optional error message. */
    public String error;
}
