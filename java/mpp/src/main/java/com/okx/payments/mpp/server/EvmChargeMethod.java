package com.okx.payments.mpp.server;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.errors.InvalidChallengeError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.charge.ChargeRequest;
import com.okx.payments.mpp.seller.ChallengeBuilder;
import com.okx.payments.mpp.seller.ChargeHandler;

import java.util.Objects;

/**
 * High-level façade implementing the {@code ChargeMethod} trait shape from D1 §6.2 and
 * Rust {@code mpp::charge::EvmChargeMethod}.
 *
 * <p>Single entry point — {@link #verifyCharge(Credential, ChargeRequest)} — auto-dispatches
 * by {@code payload.type}:
 * <ul>
 *   <li>{@code transaction} → SA {@code /charge/settle} (EIP-3009 server-broadcast)</li>
 *   <li>{@code hash}        → SA {@code /charge/verifyHash} (client-broadcast)</li>
 * </ul>
 *
 * <p>Rejects {@code authorization.type=delegation} (P2 stub).
 */
public final class EvmChargeMethod {

    private final ChargeHandler handler;
    private final ObjectMapper mapper;
    /**
     * Optional façade gating (T1-5). When non-null and {@code credential.challenge() != null},
     * {@link #verifyCharge} HMAC-verifies and time-checks the echoed challenge before dispatch.
     * Pass {@code null} for interceptor-mode setups where the routing layer pre-verifies.
     */
    private final ChallengeBuilder challengeBuilder;

    /** Interceptor-mode constructor — caller (e.g. {@code MppPaymentInterceptor}) pre-verifies. */
    public EvmChargeMethod(ChargeHandler handler) {
        this(handler, (ChallengeBuilder) null, new ObjectMapper());
    }

    /** Façade-mode constructor — {@code verifyCharge} HMAC-verifies + time-checks the challenge. */
    public EvmChargeMethod(ChargeHandler handler, ChallengeBuilder challengeBuilder) {
        this(handler, challengeBuilder, new ObjectMapper());
    }

    public EvmChargeMethod(ChargeHandler handler, ObjectMapper mapper) {
        this(handler, null, mapper);
    }

    public EvmChargeMethod(ChargeHandler handler, ChallengeBuilder challengeBuilder, ObjectMapper mapper) {
        this.handler = Objects.requireNonNull(handler);
        this.challengeBuilder = challengeBuilder;        // optional — null = interceptor mode
        this.mapper = Objects.requireNonNull(mapper);
    }

    /**
     * Auto-dispatch a charge credential by {@code payload.type}.
     *
     * @param credential decoded {@code Authorization: Payment} body
     * @param request    decoded {@code Challenge.request} (Base64UrlJson → ChargeRequest);
     *                   currently unused for dispatch (SA validates) but exposed for parity
     *                   with Rust signature and future binding-check (C1 guard)
     */
    public Receipt.ChargeReceipt verifyCharge(Credential credential, ChargeRequest request) {
        Objects.requireNonNull(credential, "credential");
        JsonNode payload = credential.payload();
        if (payload == null || payload.isNull()) {
            throw new InvalidPayloadError("credential.payload is missing");
        }
        // T1-5 façade gating. SA's charge path DOES enforce expires, but redundantly checking
        // here lets us return a clean InvalidChallengeError up-front for façade-direct callers
        // without round-tripping SA for a 70009.
        if (challengeBuilder != null && credential.challenge() != null
                && !challengeBuilder.verifyAlive(credential.challenge())) {
            throw new InvalidChallengeError(
                "challenge failed HMAC verification or has expired");
        }
        String type = payload.path("type").asText("");
        Object body = mapper.convertValue(credential, java.util.Map.class);
        return switch (type) {
            case "transaction" -> {
                String authType = payload.path("authorization").path("type").asText("");
                yield handler.verifyTransaction(body, authType);
            }
            case "hash" -> handler.verifyHash(body);
            default -> throw new InvalidPayloadError(
                "unknown charge payload type: '" + type + "' (expected transaction|hash)");
        };
    }

    /** Underlying handler — exposed for advanced callers. */
    public ChargeHandler handler() {
        return handler;
    }
}
