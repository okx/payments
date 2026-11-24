// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.eip712;

import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;

public final class AccessProofEip191 {

    private AccessProofEip191() {}

    private static final byte[] ETH_MESSAGE_PREFIX =
            "Ethereum Signed Message:\n32".getBytes(StandardCharsets.UTF_8);

    /**
     * Byte width the timestamp is packed as in abi.encodePacked: the timestamp is encoded as a
     * 32-byte big-endian integer (uint256).
     */
    private static final int TIMESTAMP_BYTES = 32;

    public static byte[] innerHash(String subId, String payer, long timestamp) {
        byte[] subIdBytes = Numeric.hexStringToByteArray(
                subId.startsWith("0x") ? subId.substring(2) : subId);
        byte[] payerBytes = Numeric.hexStringToByteArray(
                payer.startsWith("0x") ? payer.substring(2) : payer);
        byte[] tsBytes = Numeric.toBytesPadded(BigInteger.valueOf(timestamp), TIMESTAMP_BYTES);

        // abi.encodePacked(bytes32 subId, address payer, uint256 timestamp)
        byte[] packed = new byte[32 + 20 + TIMESTAMP_BYTES];
        System.arraycopy(subIdBytes, 0, packed, 0, 32);
        System.arraycopy(payerBytes, 0, packed, 32, 20);
        System.arraycopy(tsBytes, 0, packed, 52, TIMESTAMP_BYTES);

        return Hash.sha3(packed);
    }

    public static byte[] personalSignHash(byte[] innerHash) {
        // EIP-191 personal sign: keccak256("\x19" || "Ethereum Signed Message:\n32" || hash)
        byte[] encoded = new byte[1 + ETH_MESSAGE_PREFIX.length + 32];
        encoded[0] = 0x19;
        System.arraycopy(ETH_MESSAGE_PREFIX, 0, encoded, 1, ETH_MESSAGE_PREFIX.length);
        System.arraycopy(innerHash, 0, encoded, 1 + ETH_MESSAGE_PREFIX.length, 32);
        return Hash.sha3(encoded);
    }

    public static byte[] accessProofDigest(String subId, String payer, long timestamp) {
        return personalSignHash(innerHash(subId, payer, timestamp));
    }
}
