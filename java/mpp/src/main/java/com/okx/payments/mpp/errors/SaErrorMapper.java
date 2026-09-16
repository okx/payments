package com.okx.payments.mpp.errors;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Map smart-account API error code → {@link MppError} subclass.
 * Aligned with {@code BizErrorCodeEnum} on the server side and Rust mpp's
 * {@code sa_client::map_sa_error}.
 */
public final class SaErrorMapper {

    private static final Logger log = LoggerFactory.getLogger(SaErrorMapper.class);

    private SaErrorMapper() {}

    public static MppError map(int code, String detail) {
        return switch (code) {
            case 30001 -> new BadRequestError(detail);
            case 70000 -> new BadRequestError(detail);
            case 70001 -> new UnsupportedChainError(detail);
            case 70002 -> new PayerBlockedError(detail);
            case 70003 -> new InvalidPayloadError(detail);
            case 70004 -> new InvalidSignatureError(detail);
            case 70005 -> new SplitSumExceedsTotalError(detail);
            case 70006 -> new SplitCountExceededError(detail);
            case 70007 -> new TxNotConfirmedError(detail);
            case 70008 -> new ChannelClosedError(detail);
            case 70009 -> new ChallengeInvalidError(detail);
            case 70010 -> new ChannelNotFoundError(detail);
            case 70011 -> new GracePeriodTooShortError(detail);
            case 70012 -> new AmountExceedsDepositError(detail);
            case 70013 -> new VoucherDeltaTooSmallError(detail);
            case 70014 -> new ChannelClosingError(detail);
            case 70015 -> new InsufficientBalanceError(detail);
            case 8000 -> new ServiceError(detail);
            default -> {
                if (code >= 70000 && code < 80000) {
                    log.warn("SA returned unmapped MPP error code {} (detail: {}) — falling back to ServiceError",
                        code, detail);
                }
                yield new ServiceError("SA error " + code + ": " + detail);
            }
        };
    }
}
