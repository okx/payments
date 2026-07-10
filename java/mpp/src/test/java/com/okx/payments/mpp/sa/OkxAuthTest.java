package com.okx.payments.mpp.sa;

import org.junit.jupiter.api.Test;

import java.time.Clock;
import java.time.Instant;
import java.time.ZoneId;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class OkxAuthTest {

    private final SaApiConfig cfg = SaApiConfig.builder()
        .baseUrl("http://x")
        .apiKey("AK")
        .secretKey("SECRET")
        .passphrase("PASS")
        .build();
    private final Clock fixed = Clock.fixed(Instant.parse("2026-04-01T12:00:00.123Z"), ZoneId.of("UTC"));
    private final OkxAuth auth = new OkxAuth(cfg, fixed);

    @Test
    void all_required_headers_present() {
        Map<String, String> h = auth.headers("POST", "/api/v6/pay/mpp/charge/settle", "{}");
        assertThat(h)
            .containsEntry("OK-ACCESS-KEY", "AK")
            .containsEntry("OK-ACCESS-PASSPHRASE", "PASS")
            .containsKeys("OK-ACCESS-SIGN", "OK-ACCESS-TIMESTAMP")
            .containsEntry("Content-Type", "application/json");
    }

    @Test
    void timestamp_formatted_with_milliseconds() {
        Map<String, String> h = auth.headers("POST", "/x", "");
        assertThat(h.get("OK-ACCESS-TIMESTAMP")).isEqualTo("2026-04-01T12:00:00.123Z");
    }

    @Test
    void sign_is_deterministic_for_same_inputs() {
        String sig1 = auth.headers("POST", "/x", "{}").get("OK-ACCESS-SIGN");
        String sig2 = auth.headers("POST", "/x", "{}").get("OK-ACCESS-SIGN");
        assertThat(sig1).isEqualTo(sig2);
    }

    @Test
    void sign_differs_when_body_differs() {
        String s1 = auth.headers("POST", "/x", "{\"a\":1}").get("OK-ACCESS-SIGN");
        String s2 = auth.headers("POST", "/x", "{\"a\":2}").get("OK-ACCESS-SIGN");
        assertThat(s1).isNotEqualTo(s2);
    }

    @Test
    void sign_differs_when_method_differs() {
        String s1 = auth.headers("POST", "/x", null).get("OK-ACCESS-SIGN");
        String s2 = auth.headers("GET", "/x", null).get("OK-ACCESS-SIGN");
        assertThat(s1).isNotEqualTo(s2);
    }

    @Test
    void method_uppercased_in_prehash() {
        String s1 = auth.headers("post", "/x", null).get("OK-ACCESS-SIGN");
        String s2 = auth.headers("POST", "/x", null).get("OK-ACCESS-SIGN");
        assertThat(s1).isEqualTo(s2);
    }

    @Test
    void null_body_treated_as_empty_string() {
        // prehash includes "" for null body — same as prehash with "".
        String s1 = auth.headers("GET", "/x", null).get("OK-ACCESS-SIGN");
        String s2 = auth.headers("GET", "/x", "").get("OK-ACCESS-SIGN");
        assertThat(s1).isEqualTo(s2);
    }
}
