// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import com.okx.x402.model.v2.PaymentRequirements;

import java.util.Map;

/**
 * V2-aware EVM signer interface.
 * Signs EIP-3009 payloads from V2 PaymentRequirements.
 */
public interface EvmSigner extends CryptoSigner {

    /**
     * Sign EIP-3009 payload from V2 PaymentRequirements.
     *
     * @param requirements V2 payment requirements containing amount, asset, payTo, etc.
     * @return map containing "signature" and "authorization" fields
     * @throws CryptoSignException if signing fails
     */
    Map<String, Object> signPaymentRequirements(PaymentRequirements requirements)
            throws CryptoSignException;

    /**
     * Get signer's checksummed wallet address.
     *
     * @return checksummed 0x-prefixed address
     */
    String getAddress();

    /**
     * Legacy bridge - not supported for V2 signing.
     *
     * @param payload not used
     * @return never returns normally
     * @throws CryptoSignException always, with message to use signPaymentRequirements
     */
    @Override
    default String sign(Map<String, Object> payload) throws CryptoSignException {
        throw new CryptoSignException("Use signPaymentRequirements() for V2 signing");
    }
}
