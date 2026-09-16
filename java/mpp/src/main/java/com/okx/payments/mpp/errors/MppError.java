package com.okx.payments.mpp.errors;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Base type for all MPP-layer failures. Maps to RFC 9457 Problem Details on the wire.
 *
 * <p>Each subclass binds a stable {@code (status, type, title)} triple. Concrete instances
 * carry an optional {@code context} bag for diagnostics (endpoint, chainId, channelId, etc.)
 * — these are surfaced in Problem Details under the {@code context} extension.
 */
public abstract sealed class MppError extends RuntimeException
    permits BadRequestError, InvalidPayloadError, InvalidSignatureError,
            UnsupportedChainError, PayerBlockedError, SplitSumExceedsTotalError,
            SplitCountExceededError, TxNotConfirmedError, ChannelClosedError,
            ChallengeInvalidError, ChannelNotFoundError, GracePeriodTooShortError,
            AmountExceedsDepositError, VoucherDeltaTooSmallError, ChannelClosingError,
            InsufficientBalanceError,
            PaymentRequiredError, PaymentExpiredError, InvalidChallengeError,
            ServiceError, BusinessError {

    private final Map<String, Object> context = new LinkedHashMap<>();

    protected MppError(String detail) {
        super(detail);
    }

    protected MppError(String detail, Throwable cause) {
        super(detail, cause);
    }

    /** RFC 9457 problem type URI. */
    public abstract String type();

    /** RFC 9457 problem title (short, human-readable). */
    public abstract String title();

    /** HTTP status to render with. */
    public abstract int status();

    /**
     * SA / spec-defined numeric error code (e.g. 70012, 70003, 70010, 8000) — null if not applicable.
     * Subclasses override to expose their canonical code. {@link HttpSaApiClient} additionally puts
     * the actual SA-returned code into {@link #context()} as {@code saCode} for SA-routed errors.
     */
    public Integer code() {
        return null;
    }

    /** Map-style context for diagnostics. Mutates and returns this for chaining. */
    public MppError put(String key, Object value) {
        if (value != null) {
            context.put(key, value);
        }
        return this;
    }

    public Map<String, Object> context() {
        return Collections.unmodifiableMap(context);
    }

    /**
     * Render as RFC 9457 Problem Details (a plain map suitable for Jackson).
     *
     * @param challengeId optional challenge correlation; included as a top-level field if non-null.
     */
    public Map<String, Object> toProblemDetails(String challengeId) {
        Map<String, Object> p = new LinkedHashMap<>();
        p.put("type", type());
        p.put("title", title());
        p.put("status", status());
        p.put("detail", getMessage());
        if (challengeId != null) {
            p.put("challengeId", challengeId);
        }
        if (!context.isEmpty()) {
            p.put("context", new LinkedHashMap<>(context));
        }
        return p;
    }

    static final String TYPE_PREFIX = "https://paymentauth.org/problems/";
}
