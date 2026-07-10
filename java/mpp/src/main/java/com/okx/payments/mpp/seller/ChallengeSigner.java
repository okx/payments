package com.okx.payments.mpp.seller;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Base64;
import java.util.List;

/**
 * HMAC-SHA256 challenge signer/verifier — exactly matches mppx semantics:
 *
 * <pre>{@code
 * input = realm | method | intent | request | expires | (digest ?? "") | (opaque ?? "")
 * id    = base64url(HMAC-SHA256(secretKey, input)).stripTrailing("=")
 * }</pre>
 *
 * <p>Multiple keys can be provided (rotation): {@link #verify} attempts each in turn.
 * Verification uses constant-time {@link MessageDigest#isEqual}.
 */
public final class ChallengeSigner {

    private static final String SEP = "|";
    private static final String HMAC_ALG = "HmacSHA256";
    private static final Base64.Encoder ENC = Base64.getUrlEncoder().withoutPadding();
    private static final Base64.Decoder DEC = Base64.getUrlDecoder();

    private final List<byte[]> keys;

    /** Create with a single signing key. */
    public ChallengeSigner(byte[] key) {
        this(List.of(key));
    }

    /** Create with multiple keys for rotation; first is the active signer. */
    public ChallengeSigner(List<byte[]> keys) {
        if (keys == null || keys.isEmpty()) {
            throw new IllegalArgumentException("at least one HMAC key required");
        }
        this.keys = List.copyOf(keys);
    }

    /** Sign the challenge fields with the active (first) key, returning a base64url id. */
    public String sign(String realm, String method, String intent,
                       String request, String expires, String digest, String opaque) {
        return ENC.encodeToString(hmac(keys.get(0), buildInput(realm, method, intent, request, expires, digest, opaque)));
    }

    /**
     * Verify the supplied {@code id}. Tries each key (rotation); returns true if any matches
     * via constant-time comparison.
     */
    public boolean verify(String id, String realm, String method, String intent,
                          String request, String expires, String digest, String opaque) {
        if (id == null) return false;
        byte[] expected;
        try {
            expected = DEC.decode(id);
        } catch (IllegalArgumentException e) {
            return false;
        }
        byte[] input = buildInput(realm, method, intent, request, expires, digest, opaque);
        for (byte[] k : keys) {
            byte[] mac = hmac(k, input);
            if (MessageDigest.isEqual(expected, mac)) {
                return true;
            }
        }
        return false;
    }

    static byte[] buildInput(String realm, String method, String intent,
                             String request, String expires, String digest, String opaque) {
        return (s(realm) + SEP + s(method) + SEP + s(intent) + SEP
            + s(request) + SEP + s(expires) + SEP + s(digest) + SEP + s(opaque))
            .getBytes(StandardCharsets.UTF_8);
    }

    private static String s(String v) {
        return v == null ? "" : v;
    }

    private static byte[] hmac(byte[] key, byte[] input) {
        try {
            Mac mac = Mac.getInstance(HMAC_ALG);
            mac.init(new SecretKeySpec(key, HMAC_ALG));
            return mac.doFinal(input);
        } catch (Exception e) {
            throw new IllegalStateException("HMAC-SHA256 failure", e);
        }
    }
}
