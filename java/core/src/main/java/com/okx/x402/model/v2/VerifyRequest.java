// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

/** Request body for facilitator /verify endpoint (V2). */
public class VerifyRequest {
    /** Protocol version. */
    public int x402Version = 2;
    /** Payment payload from client. */
    public PaymentPayload paymentPayload;
    /** Payment requirements from server. */
    public PaymentRequirements paymentRequirements;
}
