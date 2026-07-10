package com.okx.payments.mpp.voucher;

import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Sign;

import java.nio.ByteBuffer;

/**
 * Signs a 32-byte EIP-712 digest with an EOA private key, producing a packed
 * 65-byte {@code r || s || v} signature. Web3j's {@link Sign#signMessage(byte[], ECKeyPair, boolean)}
 * with {@code needToHash=false} signs the digest directly (no Ethereum prefix).
 *
 * <p>Web3j's signer already enforces low-{@code s} (canonical form), so we don't need to
 * post-process. {@code v} comes back as {@code 27}/{@code 28} which matches EIP-712 EOA ecrecover.
 */
public final class Eip712Signer {

    private Eip712Signer() {}

    /** Sign 32-byte digest → 65-byte {@code r || s || v}. */
    public static byte[] sign(byte[] digest, ECKeyPair key) {
        if (digest == null || digest.length != 32) {
            throw new IllegalArgumentException("digest must be 32 bytes");
        }
        Sign.SignatureData sd = Sign.signMessage(digest, key, /* needToHash */ false);
        ByteBuffer buf = ByteBuffer.allocate(65);
        buf.put(sd.getR());
        buf.put(sd.getS());
        buf.put(sd.getV());     // single byte, 27 or 28 for EIP-712 EOA recovery
        return buf.array();
    }
}
