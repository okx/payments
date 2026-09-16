package com.okx.payments.mpp.voucher;

import org.web3j.crypto.Keys;
import org.web3j.crypto.Sign;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.security.SignatureException;
import java.util.Arrays;

/**
 * Verifies a packed 65-byte EIP-712 EOA signature.
 *
 * <p>Enforces:
 * <ul>
 *   <li>length == 65 (rejects EIP-2098 compact 64B form)</li>
 *   <li>{@code v} ∈ {27, 28}</li>
 *   <li>canonical low-{@code s} ({@code s &lt;= secp256k1Order/2})</li>
 *   <li>recovered EIP-55-checksummed address equals expected signer</li>
 * </ul>
 */
public final class Eip712Verifier {

    /** secp256k1 group order N from SEC2. */
    private static final BigInteger SECP256K1_N = new BigInteger(
        "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141", 16);
    private static final BigInteger SECP256K1_HALF_N = SECP256K1_N.shiftRight(1);

    private Eip712Verifier() {}

    /**
     * @param digest 32-byte EIP-712 digest
     * @param signature packed 65-byte {@code r || s || v}
     * @param expectedSigner 0x-prefixed 40-hex EIP-55 or lowercase
     * @throws Eip712VerifyException on any policy violation or recovery mismatch
     */
    public static void verify(byte[] digest, byte[] signature, String expectedSigner) {
        if (digest == null || digest.length != 32) {
            throw new Eip712VerifyException("digest must be 32 bytes");
        }
        if (signature == null || signature.length != 65) {
            throw new Eip712VerifyException(
                "signature must be exactly 65 bytes (got " + (signature == null ? "null" : signature.length) + "); EIP-2098 compact form rejected");
        }
        if (expectedSigner == null) {
            throw new Eip712VerifyException("expectedSigner must not be null");
        }
        byte[] r = Arrays.copyOfRange(signature, 0, 32);
        byte[] s = Arrays.copyOfRange(signature, 32, 64);
        byte v = signature[64];

        if (v != 27 && v != 28) {
            throw new Eip712VerifyException("v must be 27 or 28, got " + (v & 0xff));
        }
        BigInteger sBi = new BigInteger(1, s);
        if (sBi.compareTo(SECP256K1_HALF_N) > 0) {
            throw new Eip712VerifyException("non-canonical s (high-s rejected)");
        }
        if (sBi.signum() == 0 || new BigInteger(1, r).signum() == 0) {
            throw new Eip712VerifyException("r and s must be non-zero");
        }

        Sign.SignatureData sd = new Sign.SignatureData(v, r, s);
        BigInteger pubKey;
        try {
            pubKey = Sign.signedMessageHashToKey(digest, sd);
        } catch (SignatureException e) {
            throw new Eip712VerifyException("signature recovery failed: " + e.getMessage(), e);
        }
        String recovered = "0x" + Keys.getAddress(pubKey);
        if (!equalsIgnoreCase(recovered, expectedSigner)) {
            throw new Eip712VerifyException(
                "recovered signer " + recovered + " != expected " + expectedSigner);
        }
    }

    /** Recover signer address (lowercase 0x-prefixed) from digest + 65B sig — for diagnostics. */
    public static String recover(byte[] digest, byte[] signature) {
        if (signature == null || signature.length != 65) {
            throw new Eip712VerifyException("signature must be 65 bytes");
        }
        byte[] r = Arrays.copyOfRange(signature, 0, 32);
        byte[] s = Arrays.copyOfRange(signature, 32, 64);
        byte v = signature[64];
        try {
            BigInteger pubKey = Sign.signedMessageHashToKey(digest, new Sign.SignatureData(v, r, s));
            return "0x" + Keys.getAddress(pubKey);
        } catch (SignatureException e) {
            throw new Eip712VerifyException("recover failed", e);
        }
    }

    private static boolean equalsIgnoreCase(String a, String b) {
        String aa = a.startsWith("0x") ? a.substring(2) : a;
        String bb = b.startsWith("0x") ? b.substring(2) : b;
        return aa.equalsIgnoreCase(bb);
    }

    public static class Eip712VerifyException extends RuntimeException {
        public Eip712VerifyException(String msg) { super(msg); }
        public Eip712VerifyException(String msg, Throwable cause) { super(msg, cause); }
    }

    /** Convenience: hex-decoded sig variant. */
    public static void verify(byte[] digest, String signatureHex, String expectedSigner) {
        verify(digest, Numeric.hexStringToByteArray(signatureHex), expectedSigner);
    }
}
