// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model;

/**
 * Permit2 authorization parameters carried in the {@code payload} of a
 * V2 {@code PaymentPayload} for {@code exact + permit2} and {@code upto + permit2}
 * schemes.
 *
 * <p>Mirrors the {@code PermitWitnessTransferFrom} struct used by the canonical
 * Permit2 contract (Uniswap deployment) plus an additional {@code witness}
 * sub-struct that the OKX x402 Permit2 proxy verifies on-chain.
 *
 * <p>The {@code witness} field uses the {@code exact}-scheme shape by default —
 * for {@code upto} signatures the {@code facilitator} field is populated.
 */
public class Permit2Authorization {

    /** Signer / owner EOA address. */
    public String from;

    /** Token + amount permitted by Permit2. */
    public TokenPermissions permitted;

    /** Must equal the scheme's x402Permit2Proxy address. */
    public String spender;

    /** Uint256 nonce as decimal string (CSPRNG-generated). */
    public String nonce;

    /** Unix timestamp (seconds) — Permit2 signature expires after this. */
    public String deadline;

    /** Witness data verified by the x402 Permit2 proxy. */
    public Witness witness;

    public Permit2Authorization() {
    }

    /** {@code TokenPermissions(address token, uint256 amount)}. */
    public static class TokenPermissions {
        /** ERC-20 token contract address. */
        public String token;
        /** Amount in atomic units (decimal string). */
        public String amount;

        public TokenPermissions() {
        }
    }

    /**
     * Witness payload bound into the EIP-712 hash.
     *
     * <p>For the {@code exact} scheme, only {@code to} and {@code validAfter}
     * are populated. For {@code upto}, the {@code facilitator} field is also
     * populated and must match the network's advertised facilitator wallet.
     */
    public static class Witness {
        /** Destination address for funds (= {@code paymentRequirements.payTo}). */
        public String to;
        /** Facilitator wallet (upto only); null for exact. */
        public String facilitator;
        /** Unix timestamp (seconds) — payment invalid before this time. */
        public String validAfter;

        public Witness() {
        }
    }
}
