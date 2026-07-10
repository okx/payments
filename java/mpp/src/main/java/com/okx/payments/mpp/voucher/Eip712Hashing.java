package com.okx.payments.mpp.voucher;

import com.okx.payments.mpp.protocol.encoding.HexUtil;
import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;

/**
 * EIP-712 primitives for the {@code EvmPaymentChannel} domain — domain separator,
 * struct hashes for {@code Voucher}, {@code SettleAuthorization}, {@code CloseAuthorization},
 * and the final 32-byte digest.
 *
 * <p>Hand-written {@code abi.encode} over fixed-size types (bytes32, uint128, uint256, address)
 * — these are all 32 bytes left-padded so we don't need {@code DynamicArray}/{@code DynamicBytes}
 * machinery from web3j-abi. Keeps the dependency surface small and the encoding auditable.
 */
public final class Eip712Hashing {

    public static final byte[] EIP712_DOMAIN_TYPEHASH = Hash.sha3(
        "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
            .getBytes(StandardCharsets.UTF_8));

    public static final byte[] VOUCHER_TYPEHASH = Hash.sha3(
        "Voucher(bytes32 channelId,uint128 cumulativeAmount)".getBytes(StandardCharsets.UTF_8));

    public static final byte[] SETTLE_AUTH_TYPEHASH = Hash.sha3(
        ("SettleAuthorization(bytes32 channelId,uint128 cumulativeAmount,"
            + "uint256 nonce,uint256 deadline)").getBytes(StandardCharsets.UTF_8));

    public static final byte[] CLOSE_AUTH_TYPEHASH = Hash.sha3(
        ("CloseAuthorization(bytes32 channelId,uint128 cumulativeAmount,"
            + "uint256 nonce,uint256 deadline)").getBytes(StandardCharsets.UTF_8));

    private Eip712Hashing() {}

    /** Compute the EIP-712 {@code domainSeparator} for the given domain. */
    public static byte[] domainSeparator(EvmPaymentChannelDomain d) {
        byte[] nameHash = Hash.sha3(d.name().getBytes(StandardCharsets.UTF_8));
        byte[] versionHash = Hash.sha3(d.version().getBytes(StandardCharsets.UTF_8));
        return Hash.sha3(concat(
            EIP712_DOMAIN_TYPEHASH,
            pad32(nameHash),
            pad32(versionHash),
            uint256(BigInteger.valueOf(d.chainId())),
            address(d.escrowAddress())));
    }

    /** {@code structHash} for {@code Voucher(bytes32 channelId, uint128 cumulativeAmount)}. */
    public static byte[] voucherStructHash(String channelIdHex, BigInteger cumulativeAmount) {
        return Hash.sha3(concat(
            VOUCHER_TYPEHASH,
            bytes32(channelIdHex),
            uint256(cumulativeAmount)));   // uint128 ABI-encoded as left-padded 32 bytes
    }

    /** {@code structHash} for {@code SettleAuthorization}. */
    public static byte[] settleAuthStructHash(String channelIdHex,
                                              BigInteger cumulativeAmount,
                                              BigInteger nonce,
                                              BigInteger deadline) {
        return Hash.sha3(concat(
            SETTLE_AUTH_TYPEHASH,
            bytes32(channelIdHex),
            uint256(cumulativeAmount),
            uint256(nonce),
            uint256(deadline)));
    }

    /** {@code structHash} for {@code CloseAuthorization}. */
    public static byte[] closeAuthStructHash(String channelIdHex,
                                             BigInteger cumulativeAmount,
                                             BigInteger nonce,
                                             BigInteger deadline) {
        return Hash.sha3(concat(
            CLOSE_AUTH_TYPEHASH,
            bytes32(channelIdHex),
            uint256(cumulativeAmount),
            uint256(nonce),
            uint256(deadline)));
    }

    /** Final EIP-712 digest = keccak256(0x1901 || domainSeparator || structHash). */
    public static byte[] digest(byte[] domainSeparator, byte[] structHash) {
        ByteBuffer buf = ByteBuffer.allocate(2 + 32 + 32);
        buf.put((byte) 0x19);
        buf.put((byte) 0x01);
        buf.put(domainSeparator);
        buf.put(structHash);
        return Hash.sha3(buf.array());
    }

    // ── ABI encoding helpers (for fixed-size types only) ──────────────────────

    public static byte[] bytes32(String hex) {
        HexUtil.requireBytes32(hex, "channelId");
        return HexUtil.decode(hex);
    }

    public static byte[] address(String hex) {
        HexUtil.requireAddress(hex, "address");
        byte[] raw = HexUtil.decode(hex);          // 20 bytes
        byte[] out = new byte[32];
        System.arraycopy(raw, 0, out, 12, 20);
        return out;
    }

    /** Encode an unsigned BigInteger ≥ 0 as 32 big-endian bytes (left-pad zero). */
    public static byte[] uint256(BigInteger v) {
        if (v == null || v.signum() < 0) {
            throw new IllegalArgumentException("uint256 must be non-negative: " + v);
        }
        if (v.bitLength() > 256) {
            throw new IllegalArgumentException("uint256 overflow: " + v);
        }
        byte[] raw = v.toByteArray();
        byte[] out = new byte[32];
        // BigInteger.toByteArray() is signed two's-complement; for positives this may emit a
        // leading 0x00 byte that we strip when copying into the right end of the 32B slot.
        int srcOffset = raw.length > 32 ? raw.length - 32 : 0;
        int srcLen = Math.min(raw.length, 32);
        System.arraycopy(raw, srcOffset, out, 32 - srcLen, srcLen);
        return out;
    }

    public static byte[] pad32(byte[] in) {
        if (in.length == 32) return in;
        byte[] out = new byte[32];
        System.arraycopy(in, 0, out, 32 - in.length, in.length);
        return out;
    }

    public static byte[] concat(byte[]... parts) {
        int len = 0;
        for (byte[] p : parts) len += p.length;
        ByteBuffer buf = ByteBuffer.allocate(len);
        for (byte[] p : parts) buf.put(p);
        return buf.array();
    }

    /** Convenience for tests: hex of digest. */
    public static String digestHex(byte[] domainSeparator, byte[] structHash) {
        return Numeric.toHexString(digest(domainSeparator, structHash));
    }
}
