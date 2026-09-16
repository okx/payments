package com.okx.payments.mpp.protocol.encoding;

import com.fasterxml.jackson.databind.ObjectMapper;

import java.nio.charset.StandardCharsets;
import java.util.Base64;

/**
 * Base64url(JCS(JSON)) codec used for {@code PaymentChallenge.request} and
 * {@code Authorization: Payment} header bodies.
 *
 * <p>Encoding pipeline: object → Jackson JSON tree → JCS canonical bytes
 * → RFC 4648 §5 base64url without padding.
 */
public final class Base64UrlJson {

    private static final Base64.Encoder ENC = Base64.getUrlEncoder().withoutPadding();
    private static final Base64.Decoder DEC = Base64.getUrlDecoder();

    private final ObjectMapper mapper;

    public Base64UrlJson() {
        this(new ObjectMapper());
    }

    public Base64UrlJson(ObjectMapper mapper) {
        this.mapper = mapper;
    }

    /** Encode any JSON-serializable object to base64url(JCS(json)). */
    public String encode(Object value) {
        try {
            byte[] canonical = JcsCanonicalizer.canonicalize(mapper.valueToTree(value));
            return ENC.encodeToString(canonical);
        } catch (Exception e) {
            throw new IllegalArgumentException("base64UrlJson encode failed", e);
        }
    }

    /** Decode base64url(JCS(json)) into the requested type. */
    public <T> T decode(String b64, Class<T> type) {
        try {
            byte[] bytes = DEC.decode(b64);
            return mapper.readValue(new String(bytes, StandardCharsets.UTF_8), type);
        } catch (Exception e) {
            throw new IllegalArgumentException("base64UrlJson decode failed", e);
        }
    }

    /** Decode base64url(JCS(json)) into a tree. */
    public com.fasterxml.jackson.databind.JsonNode decodeTree(String b64) {
        try {
            byte[] bytes = DEC.decode(b64);
            return mapper.readTree(bytes);
        } catch (Exception e) {
            throw new IllegalArgumentException("base64UrlJson decodeTree failed", e);
        }
    }
}
