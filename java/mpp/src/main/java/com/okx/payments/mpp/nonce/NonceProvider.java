package com.okx.payments.mpp.nonce;

import java.math.BigInteger;

/**
 * Allocates a fresh uint256 nonce for {@code SettleAuthorization} / {@code CloseAuthorization}.
 *
 * <p>By design the SDK does not maintain a "used set" — the contract enforces uniqueness via
 * {@code (payee, channelId, nonce)} and reverts {@code NonceAlreadyUsed} on collision.
 * The provider only needs to produce values that won't collide in practice.
 */
@FunctionalInterface
public interface NonceProvider {

    /** Allocate a new uint256 nonce. {@code payee + channelId} are passed for reference only. */
    BigInteger allocate(String payee, String channelId);
}
