package com.okx.payments.mpp.nonce;

import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.util.UUID;

/**
 * Default {@link NonceProvider} — UUID v4 → {@code uint256}. Stateless, no persistence.
 *
 * <p>Encoding: 16 random bytes → unsigned BigInteger (max 128 bits, fits trivially in uint256).
 * Practical collision probability is negligible (≈ 5×10⁻¹¹ over 2.7×10¹⁸ nonces — UUID v4 birthday bound).
 */
public final class UuidNonceProvider implements NonceProvider {

    @Override
    public BigInteger allocate(String payee, String channelId) {
        UUID u = UUID.randomUUID();
        ByteBuffer buf = ByteBuffer.allocate(16);
        buf.putLong(u.getMostSignificantBits());
        buf.putLong(u.getLeastSignificantBits());
        return new BigInteger(1, buf.array());
    }
}
