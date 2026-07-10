// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.Map;

/** Defines one acceptable way to pay for a resource (V2 protocol). */
public class PaymentRequirements {
    /** Payment scheme identifier (e.g. "exact"). */
    public String scheme;
    /** CAIP-2 network identifier (e.g. "eip155:196"). */
    public String network;
    /** Payment amount in atomic units. */
    public String amount;
    /** Recipient wallet address. */
    public String payTo;
    /** Maximum timeout in seconds for payment validity. */
    public int maxTimeoutSeconds;
    /** Token contract address. */
    public String asset;
    /** Scheme-specific extra fields (e.g. name, version, transferMethod for EIP-3009). */
    public Map<String, Object> extra;
}
