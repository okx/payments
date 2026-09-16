package com.okx.payments.mpp.sa;

import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.format.DateTimeFormatter;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Builds OKX REST API auth headers — same scheme used by x402 OKXFacilitatorClient and
 * other OKX OpenAPI surfaces.
 *
 * <pre>{@code
 * timestamp = ISO8601(now, millisecond precision)
 * prehash   = timestamp + method + path + (body ?? "")
 * sign      = base64( HMAC-SHA256(secretKey, prehash) )
 * }</pre>
 *
 * <p>Headers attached:
 * {@code OK-ACCESS-KEY}, {@code OK-ACCESS-SIGN}, {@code OK-ACCESS-TIMESTAMP},
 * {@code OK-ACCESS-PASSPHRASE}, {@code Content-Type: application/json}.
 */
public final class OkxAuth {

    public static final String H_KEY = "OK-ACCESS-KEY";
    public static final String H_SIGN = "OK-ACCESS-SIGN";
    public static final String H_TS = "OK-ACCESS-TIMESTAMP";
    public static final String H_PASSPHRASE = "OK-ACCESS-PASSPHRASE";

    /** ISO-8601 with millisecond precision and trailing "Z" — OKX OpenAPI convention. */
    private static final DateTimeFormatter TS_FORMAT =
        DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(java.time.ZoneOffset.UTC);

    private final SaApiConfig config;
    private final java.time.Clock clock;

    public OkxAuth(SaApiConfig config) {
        this(config, java.time.Clock.systemUTC());
    }

    public OkxAuth(SaApiConfig config, java.time.Clock clock) {
        this.config = config;
        this.clock = clock;
    }

    public Map<String, String> headers(String httpMethod, String pathWithQuery, String bodyOrNull) {
        String ts = TS_FORMAT.format(Instant.now(clock));
        String body = bodyOrNull == null ? "" : bodyOrNull;
        String prehash = ts + httpMethod.toUpperCase() + pathWithQuery + body;
        String sign = sign(config.secretKey(), prehash);

        Map<String, String> h = new LinkedHashMap<>();
        h.put("Content-Type", "application/json");
        h.put(H_KEY, config.apiKey());
        h.put(H_SIGN, sign);
        h.put(H_TS, ts);
        h.put(H_PASSPHRASE, config.passphrase());
        return h;
    }

    static String sign(String secret, String prehash) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            byte[] sig = mac.doFinal(prehash.getBytes(StandardCharsets.UTF_8));
            return Base64.getEncoder().encodeToString(sig);
        } catch (Exception e) {
            throw new IllegalStateException("HMAC-SHA256 failed", e);
        }
    }
}
