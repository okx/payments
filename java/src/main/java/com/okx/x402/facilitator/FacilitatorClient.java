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
     * Retrieves the payment kinds supported by this facilitator.
     *
     * @return supported response with available kinds
     * @throws IOException if request fails
     * @throws InterruptedException if the request is interrupted
     */
    SupportedResponse supported() throws IOException, InterruptedException;
}
