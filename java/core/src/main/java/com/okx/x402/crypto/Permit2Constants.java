// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

/**
 * Canonical Permit2 addresses and EIP-712 type strings used by the
 * {@code exact + permit2} and {@code upto + permit2} schemes.
 *
 * <p>The Permit2 contract is the same CREATE2 deployment on every EVM chain
 * (Uniswap canonical address). The two OKX x402 proxy contracts differ per
 * scheme but are equally CREATE2-deployed so the same address holds on every
 * EVM chain.
 */
public final class Permit2Constants {

    private Permit2Constants() {}

    /** Canonical Permit2 contract (Uniswap) — same address on every EVM chain. */
    public static final String PERMIT2_ADDRESS =
            "0x000000000022D473030F116dDEE9F6B43aC78BA3";

    /** OKX x402 exact-Permit2 proxy address (CREATE2, same on every EVM chain). */
    public static final String EXACT_PERMIT2_PROXY_ADDRESS =
            "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";

    /** OKX x402 upto-Permit2 proxy address (CREATE2, same on every EVM chain). */
    public static final String UPTO_PERMIT2_PROXY_ADDRESS =
            "0x4020e7393B728A3939659E5732F87fdd8e680002";

    /**
     * Default safety buffer added to {@code maxTimeoutSeconds} when computing
     * the Permit2 deadline, so the deadline does not fall right at the network
     * cutoff.
     */
    public static final int PERMIT2_DEADLINE_BUFFER_SECONDS = 6;

    /** EIP-712 type string for the inner {@code TokenPermissions} struct. */
    public static final String TOKEN_PERMISSIONS_TYPE =
            "TokenPermissions(address token,uint256 amount)";

    /** EIP-712 type string for the {@code exact}-scheme witness. */
    public static final String EXACT_WITNESS_TYPE =
            "Witness(address to,uint256 validAfter)";

    /** EIP-712 type string for the {@code upto}-scheme witness (carries facilitator). */
    public static final String UPTO_WITNESS_TYPE =
            "Witness(address to,address facilitator,uint256 validAfter)";

    /** Full type string used to derive the {@code exact}-scheme typehash. */
    public static final String PERMIT_WITNESS_TRANSFER_FROM_TYPE_EXACT =
            "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,Witness witness)"
                    + TOKEN_PERMISSIONS_TYPE
                    + EXACT_WITNESS_TYPE;

    /** Full type string used to derive the {@code upto}-scheme typehash. */
    public static final String PERMIT_WITNESS_TRANSFER_FROM_TYPE_UPTO =
            "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,Witness witness)"
                    + TOKEN_PERMISSIONS_TYPE
                    + UPTO_WITNESS_TYPE;

    /** EIP-712 domain {@code name} for Permit2 (no {@code version} / {@code salt}). */
    public static final String PERMIT2_DOMAIN_NAME = "Permit2";
}
