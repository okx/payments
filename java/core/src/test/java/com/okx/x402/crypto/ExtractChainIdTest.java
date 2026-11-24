package com.okx.x402.crypto;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

/**
 * Pins {@code OKXEvmSigner.extractChainId} to a 64-bit {@code long} so chain
 * IDs above {@code Integer.MAX_VALUE} no longer throw (the integer-overflow
 * finding). EIP-155 chainId is uint256; long covers every real EVM chain.
 */
class ExtractChainIdTest {

    @Test
    void parsesStandardChainId() {
        assertEquals(196L, OKXEvmSigner.extractChainId("eip155:196"));
    }

    @Test
    void parsesChainIdAboveIntegerMaxValue() {
        // 2^31 = 2147483648 — would overflow Integer.parseInt.
        assertEquals(2147483648L, OKXEvmSigner.extractChainId("eip155:2147483648"));
    }

    @Test
    void parsesLargeChainId() {
        // A real-world-shaped large L2/L3 id well beyond 32 bits.
        assertEquals(7777777777L, OKXEvmSigner.extractChainId("eip155:7777777777"));
    }

    @Test
    void rejectsNonEip155Namespace() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXEvmSigner.extractChainId("solana:101"));
    }

    @Test
    void rejectsNonNumericId() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXEvmSigner.extractChainId("eip155:abc"));
    }
}
