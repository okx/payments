// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;

import java.io.IOException;

/**
 * Contract for calling an x402 facilitator (V2 protocol).
 * All types use V2 model classes.
 */
public interface FacilitatorClient {

    /**
     * Verifies a payment payload against the given requirements.
     *
     * @param payload the V2 payment payload to verify
     * @param requirements the payment requirements to validate against
     * @return verification response
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    VerifyResponse verify(PaymentPayload payload, PaymentRequirements requirements)
            throws IOException, InterruptedException;

    /**
     * Settles a verified payment on the blockchain.
     *
     * @param payload the V2 payment payload to settle
     * @param requirements the payment requirements for settlement
     * @return settlement response with transaction details
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    SettleResponse settle(PaymentPayload payload, PaymentRequirements requirements)
            throws IOException, InterruptedException;

    /**
     * Settles a verified payment with optional sync mode.
     *
     * @param payload the V2 payment payload to settle
     * @param requirements the payment requirements for settlement
     * @param syncSettle if true, wait for on-chain confirmation before returning
     * @return settlement response with transaction details and status
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    default SettleResponse settle(PaymentPayload payload,
                                  PaymentRequirements requirements,
                                  boolean syncSettle)
            throws IOException, InterruptedException {
        return settle(payload, requirements);
    }

    /**
     * Queries the settlement status of a previously submitted transaction.
     * Used for async polling when syncSettle=false.
     *
     * @param txHash the on-chain transaction hash to check
     * @return settlement response with current status
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    SettleResponse settleStatus(String txHash) throws IOException, InterruptedException;

    /**
     * Retrieves the payment kinds supported by this facilitator.
     *
     * @return supported response with available kinds
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    SupportedResponse supported() throws IOException, InterruptedException;
}
