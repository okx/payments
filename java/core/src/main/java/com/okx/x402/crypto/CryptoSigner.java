// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import java.util.Map;

/**
 * Produces a protocol-specific signature for an x402 payment-authorization payload.
 *
 * <p>Each implementation interprets the payload keys as defined by its
 * payment scheme and returns the scheme's canonical encoding:</p>
 *
 * <ul>
 *   <li><b>exact-evm</b> - ERC-3009 transferWithAuthorization.
 *       Return: 0x-prefixed 65-byte hex string.</li>
 *   <li><b>exact-solana</b> - Ed25519 over the canonical JSON payload.
 *       Return: Base58-encoded 64-byte signature.</li>
 * </ul>
 */
public interface CryptoSigner {

    /**
     * Signs the supplied payload and returns the signature.
     *
     * @param payload scheme-specific authorization fields
     * @return encoded signature string
     * @throws CryptoSignException for cryptographic failures
     */
    String sign(Map<String, Object> payload) throws CryptoSignException;
}
