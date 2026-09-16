// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v1;

import java.util.Map;

/**
 * Describes the payment requirements for accessing a resource (v1).
 */
public class PaymentRequirements {

    /** The payment scheme identifier. */
    public String scheme;

    /** The blockchain network. */
    public String network;

    /** The maximum amount required for payment. */
    public String maxAmountRequired;

    /** The resource URI. */
    public String resource;

    /** A human-readable description of the resource. */
    public String description;

    /** The MIME type of the resource. */
    public String mimeType;

    /** The JSON schema describing the response output. */
    public Map<String, Object> outputSchema;

    /** The address to send payment to. */
    public String payTo;

    /** The maximum timeout in seconds for the payment. */
    public int maxTimeoutSeconds;

    /** The asset address or identifier. */
    public String asset;

    /** Extra scheme-specific parameters. */
    public Map<String, Object> extra;
}
