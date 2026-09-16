package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.charge.ChargeRequest;
import com.okx.payments.mpp.protocol.charge.ChargeMethodDetails;
import org.junit.jupiter.api.Test;

import java.time.Clock;
import java.time.Duration;
import java.time.Instant;
import java.time.ZoneId;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class ChallengeBuilderTest {

    private final ChallengeSigner signer = new ChallengeSigner("test-key-deadbeef".getBytes());
    private final Clock fixedClock = Clock.fixed(Instant.parse("2026-04-01T12:00:00Z"), ZoneId.of("UTC"));
    private final ChallengeBuilder builder = new ChallengeBuilder(
        signer,
        new com.okx.payments.mpp.protocol.encoding.Base64UrlJson(),
        fixedClock,
        Duration.ofMinutes(5));

    @Test
    void builds_signed_challenge_for_charge_request() {
        ChargeRequest req = new ChargeRequest("10000",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            null, null,
            new ChargeMethodDetails(196L, false, null, null, null));
        Challenge c = builder.build("api.example.com", Method.EVM, Intent.CHARGE, req);

        assertThat(c.id()).isNotBlank();
        assertThat(c.realm()).isEqualTo("api.example.com");
        assertThat(c.method()).isEqualTo(Method.EVM);
        assertThat(c.intent()).isEqualTo(Intent.CHARGE);
        assertThat(c.request()).isNotBlank();
        assertThat(c.expires()).isEqualTo("2026-04-01T12:05:00Z");
        // Roundtrip via verify
        assertThat(builder.verify(c)).isTrue();
    }

    @Test
    void same_request_produces_same_id_under_fixed_clock() {
        // Order-insensitive payload (Map insertion order varies but JCS sorts) → stable id.
        Map<String, Object> req = Map.of("amount", "10000", "currency", "0xab");
        Challenge a = builder.build("realm", Method.EVM, Intent.CHARGE, req);
        Challenge b = builder.build("realm", Method.EVM, Intent.CHARGE, req);
        assertThat(a.id()).isEqualTo(b.id());
    }

    @Test
    void verify_rejects_tampered_request() {
        Challenge c = builder.build("realm", Method.EVM, Intent.CHARGE, Map.of("amount", "100"));
        Challenge tampered = new Challenge(c.id(), c.realm(), c.method(), c.intent(),
            "DIFFERENT_REQUEST", c.expires(), c.description(), c.digest(), c.opaque());
        assertThat(builder.verify(tampered)).isFalse();
    }

    @Test
    void custom_ttl_changes_expires() {
        Challenge c = builder.build("realm", Method.EVM, Intent.SESSION,
            Map.of("amount", "1"), Duration.ofMinutes(10));
        assertThat(c.expires()).isEqualTo("2026-04-01T12:10:00Z");
    }

    @Test
    void verify_null_returns_false() {
        assertThat(builder.verify(null)).isFalse();
    }

    // ── T1-3: verifyAlive HMAC + expires check ────────────────────────────────

    @Test
    void verifyAlive_returns_true_for_fresh_challenge() {
        Challenge c = builder.build("realm", Method.EVM, Intent.SESSION,
            Map.of("amount", "1"));      // expires = clock + 5min, well ahead of clock
        assertThat(builder.verifyAlive(c)).isTrue();
    }

    @Test
    void verifyAlive_returns_false_for_expired_challenge() {
        // Build a challenge under one clock, then verify under a later one.
        Clock pastClock = Clock.fixed(Instant.parse("2026-04-01T11:50:00Z"), ZoneId.of("UTC"));
        ChallengeBuilder pastBuilder = new ChallengeBuilder(
            signer, new com.okx.payments.mpp.protocol.encoding.Base64UrlJson(),
            pastClock, Duration.ofMinutes(5));         // expires = 2026-04-01T11:55:00Z

        Challenge stale = pastBuilder.build("realm", Method.EVM, Intent.SESSION,
            Map.of("amount", "1"));
        // Builder uses fixedClock = 2026-04-01T12:00:00Z, which is past 11:55 expiry.
        assertThat(builder.verifyAlive(stale)).isFalse();
        // HMAC alone still passes — the only difference is the expires check.
        assertThat(builder.verify(stale)).isTrue();
    }

    @Test
    void verifyAlive_returns_false_for_unparseable_expires() {
        // Construct a challenge with a deliberately unparseable expires, but valid HMAC.
        Challenge raw = builder.build("realm", Method.EVM, Intent.SESSION, Map.of("amount", "1"));
        String badExpires = "not-a-timestamp";
        String id = signer.sign(raw.realm(), raw.method().wire(), raw.intent().wire(),
            raw.request(), badExpires, raw.digest(), raw.opaque());
        Challenge tampered = new Challenge(id, raw.realm(), raw.method(), raw.intent(),
            raw.request(), badExpires, raw.description(), raw.digest(), raw.opaque());

        assertThat(builder.verify(tampered)).isTrue();             // HMAC OK
        assertThat(builder.verifyAlive(tampered)).isFalse();       // unparseable = forgery signal
    }

    @Test
    void verifyAlive_returns_true_for_blank_expires() {
        // Open-ended challenge (no expires) — HMAC alone is the contract.
        Challenge raw = builder.build("realm", Method.EVM, Intent.SESSION, Map.of("amount", "1"));
        String id = signer.sign(raw.realm(), raw.method().wire(), raw.intent().wire(),
            raw.request(), null, raw.digest(), raw.opaque());
        Challenge openEnded = new Challenge(id, raw.realm(), raw.method(), raw.intent(),
            raw.request(), null, raw.description(), raw.digest(), raw.opaque());
        assertThat(builder.verifyAlive(openEnded)).isTrue();
    }

    @Test
    void verifyAlive_returns_false_for_null_challenge() {
        assertThat(builder.verifyAlive(null)).isFalse();
    }

    @Test
    void verifyAlive_returns_false_when_hmac_fails() {
        Challenge c = builder.build("realm", Method.EVM, Intent.SESSION, Map.of("amount", "1"));
        Challenge tampered = new Challenge(c.id(), c.realm(), c.method(), c.intent(),
            "DIFFERENT_REQUEST", c.expires(), c.description(), c.digest(), c.opaque());
        assertThat(builder.verifyAlive(tampered)).isFalse();
    }
}
