// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.VerifyResponse;

/**
 * Lifecycle hook interfaces for PaymentFilter.
 *
 * <p>Matches the hook system in Go/TypeScript SDKs:
 * <ul>
 *   <li>Before-hooks can abort the flow with a reason</li>
 *   <li>After-hooks are fire-and-forget notifications</li>
 *   <li>Failure-hooks can recover from errors with an override result</li>
 * </ul>
 */
public final class PaymentHooks {

    private PaymentHooks() {
    }

    /** Result from a before-hook that can abort the payment flow. */
    public static class AbortResult {
        /** Whether to abort. */
        public final boolean abort;
        /** Machine-readable reason for aborting (sent as 402 error). */
        public final String reason;

        private AbortResult(boolean abort, String reason) {
            this.abort = abort;
            this.reason = reason;
        }

        /** Continue the payment flow normally. */
        public static AbortResult proceed() {
            return new AbortResult(false, null);
        }

        /** Abort the payment flow, returning 402 with the given reason. */
        public static AbortResult abort(String reason) {
            return new AbortResult(true, reason);
        }
    }

    /** Result from a failure-hook that can recover with an override response. */
    public static class RecoverResult<T> {
        /** Whether recovery succeeded. */
        public final boolean recovered;
        /** Override result to use if recovered. */
        public final T result;

        private RecoverResult(boolean recovered, T result) {
            this.recovered = recovered;
            this.result = result;
        }

        /** Indicate that recovery was not possible. */
        public static <T> RecoverResult<T> notRecovered() {
            return new RecoverResult<>(false, null);
        }

        /** Provide a recovered result to continue the flow. */
        public static <T> RecoverResult<T> recovered(T result) {
            return new RecoverResult<>(true, result);
        }
    }

    /** Called before facilitator.verify(). Can abort with a reason. */
    @FunctionalInterface
    public interface BeforeVerifyHook {
        AbortResult beforeVerify(PaymentPayload payload, PaymentRequirements requirements);
    }

    /** Called after verify succeeds (isValid=true). Fire-and-forget. */
    @FunctionalInterface
    public interface AfterVerifyHook {
        void afterVerify(PaymentPayload payload, PaymentRequirements requirements,
                         VerifyResponse result);
    }

    /** Called when verify throws an exception. Can recover with an override. */
    @FunctionalInterface
    public interface OnVerifyFailureHook {
        RecoverResult<VerifyResponse> onVerifyFailure(
                PaymentPayload payload, PaymentRequirements requirements, Exception error);
    }

    /** Called before facilitator.settle(). Can abort with a reason. */
    @FunctionalInterface
    public interface BeforeSettleHook {
        AbortResult beforeSettle(PaymentPayload payload, PaymentRequirements requirements);
    }

    /** Called after settle succeeds (success=true). Fire-and-forget. */
    @FunctionalInterface
    public interface AfterSettleHook {
        void afterSettle(PaymentPayload payload, PaymentRequirements requirements,
                         SettleResponse result);
    }

    /** Called when settle fails (success=false or exception). Can recover. */
    @FunctionalInterface
    public interface OnSettleFailureHook {
        RecoverResult<SettleResponse> onSettleFailure(
                PaymentPayload payload, PaymentRequirements requirements, Exception error);
    }

    // -----------------------------------------------------------------------
    // HTTP-layer hooks (match TS x402HTTPResourceServer)
    // -----------------------------------------------------------------------

    /**
     * Result from {@link OnProtectedRequestHook}: either continue with the
     * normal payment flow, bypass payment entirely, or abort with a reason.
     *
     * <p>Matches the TS return shape
     * {@code void | { grantAccess: true } | { abort: true; reason }}.
     */
    public static final class ProtectedRequestResult {

        public enum Decision { PROCEED, GRANT_ACCESS, ABORT }

        public final Decision decision;
        /** Non-null only when {@code decision == ABORT}. */
        public final String reason;

        private ProtectedRequestResult(Decision decision, String reason) {
            this.decision = decision;
            this.reason = reason;
        }

        /** Continue to the normal payment verification flow. */
        public static ProtectedRequestResult proceed() {
            return new ProtectedRequestResult(Decision.PROCEED, null);
        }

        /** Bypass payment — treat the request as if no payment were required. */
        public static ProtectedRequestResult grantAccess() {
            return new ProtectedRequestResult(Decision.GRANT_ACCESS, null);
        }

        /** Reject the request early with HTTP 403 and the given reason. */
        public static ProtectedRequestResult abort(String reason) {
            return new ProtectedRequestResult(Decision.ABORT, reason);
        }
    }

    /**
     * Runs after route match and before the payment header is read. Lets the
     * application grant access without payment (e.g. API-key tier) or reject
     * a request outright. Matches the TS {@code onProtectedRequest} hook.
     *
     * <p>Multiple hooks may be registered; they run in registration order and
     * the first hook returning {@code GRANT_ACCESS} or {@code ABORT} wins.
     */
    @FunctionalInterface
    public interface OnProtectedRequestHook {
        ProtectedRequestResult onProtectedRequest(
                X402Request request, PaymentProcessor.RouteConfig routeConfig);
    }

    /**
     * Result from {@link OnSettlementTimeoutHook}. Matches the TS return shape
     * {@code { confirmed: boolean }}.
     */
    public static final class SettlementTimeoutResult {

        public final boolean confirmed;

        private SettlementTimeoutResult(boolean confirmed) {
            this.confirmed = confirmed;
        }

        /** The tx did in fact confirm out-of-band; treat settlement as success. */
        public static SettlementTimeoutResult confirmed() {
            return new SettlementTimeoutResult(true);
        }

        /** The tx did not confirm; fall through to the normal timeout 402. */
        public static SettlementTimeoutResult notConfirmed() {
            return new SettlementTimeoutResult(false);
        }
    }

    /**
     * Called once when settle-status polling exhausts its deadline without a
     * definitive result. Single-hook: last registration wins. Exceptions
     * thrown by the hook are caught and treated as {@code notConfirmed()}.
     */
    @FunctionalInterface
    public interface OnSettlementTimeoutHook {
        SettlementTimeoutResult onSettlementTimeout(String txHash, String network);
    }
}
