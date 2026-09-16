// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.access;

import com.okx.x402.subscription.eip712.AccessProofEip191;
import com.okx.x402.subscription.model.AccessProof;
import com.okx.x402.util.Json;
import org.web3j.crypto.ECDSASignature;
import org.web3j.crypto.Keys;
import org.web3j.crypto.Sign;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Base64;

/**
 * Stateless AccessProof (X-APP-Access header) verification: base64 decode → time window →
 * EIP-191 ecrecover == payer. Shared by {@link AccessProofHook} and the /changePlan route.
 */
public final class AccessProofVerifier {

    /** Canonical access header (no X- prefix). */
    public static final String HEADER_APP_ACCESS = "APP-Access";
    /** Legacy header name (earlier releases); still read inbound. */
    public static final String LEGACY_HEADER_APP_ACCESS = "X-APP-Access";
    public static final int DEFAULT_TIME_WINDOW_SECONDS = 300;

    /** Read the access header, canonical name first, legacy alias second. */
    public static String readAccessHeader(com.okx.x402.server.X402Request request) {
        String v = request.getHeader(HEADER_APP_ACCESS);
        if (v == null || v.isEmpty()) {
            v = request.getHeader(LEGACY_HEADER_APP_ACCESS);
        }
        return v;
    }

    private final int timeWindowSeconds;

    public AccessProofVerifier() {
        this(DEFAULT_TIME_WINDOW_SECONDS);
    }

    public AccessProofVerifier(int timeWindowSeconds) {
        this.timeWindowSeconds = timeWindowSeconds;
    }

    /** Decode the base64 X-APP-Access header; null on malformed input. */
    public AccessProof decode(String headerValue) {
        if (headerValue == null || headerValue.isEmpty()) {
            return null;
        }
        try {
            // Tolerate surrounding whitespace from header middleboxes.
            String json = new String(Base64.getDecoder().decode(headerValue.trim()),
                    StandardCharsets.UTF_8);
            return Json.MAPPER.readValue(json, AccessProof.class);
        } catch (Exception e) {
            return null;
        }
    }

    /**
     * Verify kind + time window + signature. Returns null when valid, otherwise a
     * machine-readable rejection reason.
     */
    public String verify(AccessProof proof) {
        // Only the subscription-id kind is accepted. A missing kind
        // keeps the model default (tolerated); an explicit wrong kind is rejected.
        if (!"subscription-id".equals(proof.kind)) {
            return "invalid access proof kind";
        }
        long now = System.currentTimeMillis() / 1000;
        if (Math.abs(now - proof.timestamp) > timeWindowSeconds) {
            return "access proof expired";
        }
        String recovered = recoverSigner(proof);
        if (recovered == null || !recovered.equalsIgnoreCase(proof.payer)) {
            return "payer mismatch";
        }
        return null;
    }

    private String recoverSigner(AccessProof proof) {
        try {
            byte[] innerHash = AccessProofEip191.innerHash(proof.subId, proof.payer, proof.timestamp);
            byte[] msgHash = AccessProofEip191.personalSignHash(innerHash);

            byte[] sigBytes = Numeric.hexStringToByteArray(
                    proof.signature.startsWith("0x") ? proof.signature.substring(2) : proof.signature);
            // Exactly 65 bytes, v ∈ {0, 1, 27, 28} — a longer signature must never be
            // silently truncated, and v=2..26 must not be mis-mapped into recovery ids.
            if (sigBytes.length != 65) {
                return null;
            }
            int v = sigBytes[64] & 0xFF;
            if (v != 0 && v != 1 && v != 27 && v != 28) {
                return null;
            }
            if (v < 27) v += 27;

            BigInteger r = new BigInteger(1, Arrays.copyOfRange(sigBytes, 0, 32));
            BigInteger s = new BigInteger(1, Arrays.copyOfRange(sigBytes, 32, 64));

            BigInteger publicKey = Sign.recoverFromSignature(v - 27,
                    new ECDSASignature(r, s), msgHash);
            if (publicKey == null) return null;
            return "0x" + Keys.getAddress(publicKey);
        } catch (Exception e) {
            return null;
        }
    }
}
