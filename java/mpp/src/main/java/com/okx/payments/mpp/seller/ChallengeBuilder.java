package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;

import java.time.Clock;
import java.time.Duration;
import java.time.Instant;
import java.time.OffsetDateTime;
import java.time.format.DateTimeFormatter;
import java.time.format.DateTimeParseException;
import java.util.Objects;

/**
 * Composes a fully-signed {@link Challenge} given a request payload object —
 * encodes the request via {@link Base64UrlJson} and signs the id via {@link ChallengeSigner}.
 */
public final class ChallengeBuilder {

    private final ChallengeSigner signer;
    private final Base64UrlJson b64Json;
    private final Clock clock;
    private final Duration defaultTtl;

    public ChallengeBuilder(ChallengeSigner signer) {
        this(signer, new Base64UrlJson(), Clock.systemUTC(), Duration.ofMinutes(5));
    }

    public ChallengeBuilder(ChallengeSigner signer, Base64UrlJson b64Json, Clock clock, Duration defaultTtl) {
        this.signer = Objects.requireNonNull(signer);
        this.b64Json = Objects.requireNonNull(b64Json);
        this.clock = Objects.requireNonNull(clock);
        this.defaultTtl = Objects.requireNonNull(defaultTtl);
    }

    public Challenge build(String realm, Method method, Intent intent, Object requestPayload) {
        return build(realm, method, intent, requestPayload, defaultTtl);
    }

    public Challenge build(String realm, Method method, Intent intent,
                           Object requestPayload, Duration ttl) {
        return build(realm, method, intent, requestPayload, ttl, null, null, null);
    }

    public Challenge build(String realm, Method method, Intent intent,
                           Object requestPayload, Duration ttl,
                           String description, String digest, String opaque) {
        Objects.requireNonNull(realm);
        Objects.requireNonNull(method);
        Objects.requireNonNull(intent);
        Objects.requireNonNull(requestPayload);

        String requestB64 = b64Json.encode(requestPayload);
        Instant exp = Instant.now(clock).plus(ttl);
        String expires = DateTimeFormatter.ISO_INSTANT.format(exp.atZone(java.time.ZoneOffset.UTC).toInstant());

        String id = signer.sign(realm, method.wire(), intent.wire(),
            requestB64, expires, digest, opaque);

        return Challenge.builder()
            .id(id)
            .realm(realm)
            .method(method)
            .intent(intent)
            .request(requestB64)
            .expires(expires)
            .description(description)
            .digest(digest)
            .opaque(opaque)
            .build();
    }

    /** Verify provenance of an echoed challenge (Tier-1 HMAC check). */
    public boolean verify(Challenge challenge) {
        if (challenge == null) return false;
        return signer.verify(challenge.id(),
            challenge.realm(),
            challenge.method() == null ? null : challenge.method().wire(),
            challenge.intent() == null ? null : challenge.intent().wire(),
            challenge.request(),
            challenge.expires(),
            challenge.digest(),
            challenge.opaque());
    }

    /**
     * HMAC-verify <strong>and</strong> temporal-check the echoed challenge.
     *
     * <p>Returns {@code true} only when both checks pass. SA's {@code MppSessionService} does NOT
     * enforce {@code Challenge.expires} on session paths (only {@code MppChargeService
     * .guardChallengeNotExpired:178} does, and only for charge) — so the SDK is the only line
     * of defence against replay of an HMAC-valid but stale session challenge. T1-3 introduces
     * this helper; callers behind {@code MppPaymentInterceptor} pick it up via T1-5 façade
     * gating.
     *
     * <p>An unparseable {@code expires} is treated as a forgery signal (returns false).
     * A null/blank {@code expires} is accepted — open-ended challenges remain HMAC-only.
     */
    public boolean verifyAlive(Challenge challenge) {
        if (!verify(challenge)) {
            return false;
        }
        String expires = challenge.expires();
        if (expires == null || expires.isBlank()) {
            return true;        // open-ended challenge — HMAC alone is the contract
        }
        try {
            Instant exp = OffsetDateTime.parse(expires).toInstant();
            return !Instant.now(clock).isAfter(exp);
        } catch (DateTimeParseException e) {
            return false;
        }
    }
}
