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
}
