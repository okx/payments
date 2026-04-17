package com.okx.x402.server;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;

import java.math.BigInteger;
import java.util.Map;

/**
 * Cross-checks a client-signed {@link PaymentPayload} against the merchant's
 * server-side {@link PaymentRequirements} before the facilitator is called.
 *
 * <p>The facilitator performs authoritative on-chain verification, but the
 * client can still mis-declare the {@code accepted} block or submit an
 * EIP-3009 authorization that points somewhere other than the configured
 * recipient. This validator makes the SDK refuse such payloads locally so
 * that a single bug on the facilitator cannot translate into lost funds or
 * wrong-merchant settlement.
 */
final class PaymentPayloadValidator {

    /** Small clock-skew tolerance, in seconds, applied to validBefore. */
    private static final long CLOCK_SKEW_SECONDS = 5L;

    private PaymentPayloadValidator() {
    }

    /**
     * Validate that a payload matches the merchant's requirements.
     *
     * @param payload      client-submitted payload decoded from the header
     * @param requirements merchant-configured requirements for this route
     * @return {@code null} if the payload is consistent with the
     *         requirements; otherwise a short machine-readable reason to
     *         return with the 402 response
     */
    static String validate(PaymentPayload payload,
                           PaymentRequirements requirements) {
        if (payload == null || requirements == null) {
            return "missing payload or requirements";
        }

        PaymentRequirements chosen = payload.accepted;
        if (chosen == null) {
            return "payload.accepted missing";
        }
        if (!equalsIgnoreCase(chosen.scheme, requirements.scheme)) {
            return "scheme mismatch";
        }
        if (!equalsIgnoreCase(chosen.network, requirements.network)) {
            return "network mismatch";
        }
        if (!equalsIgnoreCase(chosen.asset, requirements.asset)) {
            return "asset mismatch";
        }
        if (!equalsIgnoreCase(chosen.payTo, requirements.payTo)) {
            return "payTo mismatch";
        }
        if (!equalsAmount(chosen.amount, requirements.amount)) {
            return "amount mismatch";
        }

        Map<String, Object> inner = payload.payload;
        if (inner == null) {
            return "signed payload missing";
        }
        Object authObj = inner.get("authorization");
        if (!(authObj instanceof Map)) {
            return "authorization missing";
        }
        Map<?, ?> auth = (Map<?, ?>) authObj;

        if (!equalsIgnoreCase(asString(auth.get("to")), requirements.payTo)) {
            return "authorization.to != payTo";
        }
        if (!equalsAmount(asString(auth.get("value")), requirements.amount)) {
            return "authorization.value != amount";
        }

        BigInteger validAfter = parse(asString(auth.get("validAfter")));
        BigInteger validBefore = parse(asString(auth.get("validBefore")));
        if (validAfter == null || validBefore == null) {
            return "validAfter/validBefore missing";
        }
        long now = System.currentTimeMillis() / 1000L;
        if (validAfter.longValueExact() > now) {
            return "authorization not yet valid";
        }
        if (validBefore.longValueExact() <= now + CLOCK_SKEW_SECONDS) {
            return "authorization expired";
        }
        return null;
    }

    private static boolean equalsIgnoreCase(String a, String b) {
        return a != null && b != null && a.equalsIgnoreCase(b);
    }

    private static boolean equalsAmount(String a, String b) {
        if (a == null || b == null) {
            return false;
        }
        try {
            return new BigInteger(a).equals(new BigInteger(b));
        } catch (NumberFormatException e) {
            return false;
        }
    }

    private static String asString(Object o) {
        return o == null ? null : o.toString();
    }

    private static BigInteger parse(String s) {
        if (s == null) {
            return null;
        }
        try {
            return new BigInteger(s);
        } catch (NumberFormatException e) {
            return null;
        }
    }
}
