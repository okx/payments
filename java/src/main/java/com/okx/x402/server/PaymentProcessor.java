package com.okx.x402.server;

import com.okx.x402.config.AssetRegistry;
import com.okx.x402.config.ResolvedPrice;
import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.SettlementResponseHeader;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequired;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.VerifyResponse;
import com.okx.x402.util.Json;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.Executor;

/**
 * Core payment processing logic shared by {@link PaymentFilter} and
 * {@link PaymentInterceptor}.
 *
 * <p>Holds all configuration, hooks, and business logic. The filter and
 * interceptor are thin adapters that delegate to this class.
 */
public class PaymentProcessor {

    static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    static final String HEADER_X_PAYMENT = "X-PAYMENT";
    static final String HEADER_SETTLEMENT_STATUS = "X-SETTLEMENT-STATUS";

    /**
     * Request attribute key. Set to {@code Boolean.FALSE} by an upstream
     * filter/interceptor to indicate this request is free (no payment needed).
     * When absent or {@code true}, payment is required as normal.
     */
    public static final String PAYMENT_REQUIRED_ATTRIBUTE = "paymentRequired";

    public static final Duration DEFAULT_POLL_INTERVAL = Duration.ofSeconds(1);
    public static final Duration DEFAULT_POLL_DEADLINE = Duration.ofSeconds(5);

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;
    private Duration pollInterval = DEFAULT_POLL_INTERVAL;
    private Duration pollDeadline = DEFAULT_POLL_DEADLINE;
    private OnSettlementTimeoutHook timeoutHook;

    private final List<PaymentHooks.BeforeVerifyHook> beforeVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterVerifyHook> afterVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.OnVerifyFailureHook> verifyFailureHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeSettleHook> beforeSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterSettleHook> afterSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.OnSettleFailureHook> settleFailureHooks = new ArrayList<>();

    private PaymentRequiredPredicate paymentRequiredPredicate;
    private Executor settleExecutor;
    private AsyncSettleCallback asyncSettleCallback;

    // -----------------------------------------------------------------------
    // Nested types
    // -----------------------------------------------------------------------

    @FunctionalInterface
    public interface DynamicPrice {
        String resolve(HttpServletRequest request);
    }

    @FunctionalInterface
    public interface OnSettlementTimeoutHook {
        boolean onTimeout(String txHash, String network);
    }

    @FunctionalInterface
    public interface AsyncSettleCallback {
        void onResult(PaymentPayload payload, PaymentRequirements requirements,
                      SettleResponse result, Throwable error);
    }

    /**
     * Predicate that determines whether payment is required for a request.
     * Return {@code false} to skip payment (free access).
     */
    @FunctionalInterface
    public interface PaymentRequiredPredicate {
        boolean isRequired(HttpServletRequest request);
    }

    public static class RouteConfig {
        public String scheme = "exact";
        public String network;
        public String payTo;
        public String price;
        public String asset;
        public int maxTimeoutSeconds = 86400;
        public boolean syncSettle;
        public DynamicPrice priceFunction;
        public boolean asyncSettle;
    }

    /** Result of the verify phase, passed from preHandle to postHandle. */
    public static class VerifyResult {
        /** Sentinel: not a paid route or skipped — caller should pass through. */
        static final VerifyResult PASS_THROUGH = new VerifyResult(null, null, null);

        public final PaymentPayload payload;
        public final PaymentRequirements requirements;
        public final RouteConfig config;

        VerifyResult(PaymentPayload payload, PaymentRequirements requirements,
                     RouteConfig config) {
            this.payload = payload;
            this.requirements = requirements;
            this.config = config;
        }

        /** True if this is a real verify result (not a pass-through sentinel). */
        public boolean isVerified() {
            return this != PASS_THROUGH;
        }
    }

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    public PaymentProcessor(FacilitatorClient facilitator,
                            Map<String, RouteConfig> routes) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.routes = Objects.requireNonNull(routes);
    }

    // -----------------------------------------------------------------------
    // Fluent configuration
    // -----------------------------------------------------------------------

    public PaymentProcessor pollInterval(Duration interval) {
        this.pollInterval = Objects.requireNonNull(interval);
        return this;
    }

    public PaymentProcessor pollDeadline(Duration deadline) {
        this.pollDeadline = Objects.requireNonNull(deadline);
        return this;
    }

    public PaymentProcessor onSettlementTimeout(OnSettlementTimeoutHook hook) {
        this.timeoutHook = hook;
        return this;
    }

    public PaymentProcessor onBeforeVerify(PaymentHooks.BeforeVerifyHook hook) {
        beforeVerifyHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor onAfterVerify(PaymentHooks.AfterVerifyHook hook) {
        afterVerifyHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor onVerifyFailure(PaymentHooks.OnVerifyFailureHook hook) {
        verifyFailureHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor onBeforeSettle(PaymentHooks.BeforeSettleHook hook) {
        beforeSettleHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor onAfterSettle(PaymentHooks.AfterSettleHook hook) {
        afterSettleHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor onSettleFailure(PaymentHooks.OnSettleFailureHook hook) {
        settleFailureHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    public PaymentProcessor paymentRequiredWhen(PaymentRequiredPredicate predicate) {
        this.paymentRequiredPredicate = Objects.requireNonNull(predicate);
        return this;
    }

    public PaymentProcessor settleExecutor(Executor executor) {
        this.settleExecutor = Objects.requireNonNull(executor);
        return this;
    }

    public PaymentProcessor onAsyncSettleComplete(AsyncSettleCallback callback) {
        this.asyncSettleCallback = Objects.requireNonNull(callback);
        return this;
    }

    // -----------------------------------------------------------------------
    // Core operations (called by Filter / Interceptor adapters)
    // -----------------------------------------------------------------------

    /**
     * Pre-handler phase: route match, skip check, header decode, verify.
     *
     * @return non-null {@link VerifyResult} if handler should proceed;
     *         null if response was already written (402 / 500)
     */
    public VerifyResult preHandle(HttpServletRequest request,
                                  HttpServletResponse response)
            throws IOException {

        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            config = routes.get(request.getRequestURI());
        }
        if (config == null) {
            return VerifyResult.PASS_THROUGH;
        }

        if (!isPaymentRequired(request)) {
            return VerifyResult.PASS_THROUGH;
        }

        String header = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (header == null || header.isEmpty()) {
            header = request.getHeader(HEADER_X_PAYMENT);
        }
        if (header == null || header.isEmpty()) {
            respond402(response, request, config, null);
            return null;
        }

        PaymentPayload payload;
        try {
            payload = PaymentPayload.fromHeader(header);
        } catch (Exception e) {
            respond402(response, request, config, "malformed payment header");
            return null;
        }

        if (payload.resource != null && payload.resource.url != null) {
            String requestUrl = request.getRequestURL().toString();
            if (!payload.resource.url.equals(requestUrl)) {
                respond402(response, request, config, "resource mismatch");
                return null;
            }
        }

        PaymentRequirements requirements = buildRequirements(request, config);

        for (PaymentHooks.BeforeVerifyHook hook : beforeVerifyHooks) {
            PaymentHooks.AbortResult ar = hook.beforeVerify(payload, requirements);
            if (ar != null && ar.abort) {
                respond402(response, request, config, ar.reason);
                return null;
            }
        }

        VerifyResponse vr;
        try {
            vr = facilitator.verify(payload, requirements);
        } catch (Exception e) {
            for (PaymentHooks.OnVerifyFailureHook hook : verifyFailureHooks) {
                PaymentHooks.RecoverResult<VerifyResponse> rr =
                        hook.onVerifyFailure(payload, requirements, e);
                if (rr != null && rr.recovered && rr.result != null) {
                    vr = rr.result;
                    if (!vr.isValid) {
                        respond402(response, request, config, vr.invalidReason);
                        return null;
                    }
                    break;
                }
            }
            if (e instanceof IOException) {
                response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json");
                response.getWriter().write(
                        "{\"error\":\"Payment verification failed: "
                                + e.getMessage() + "\"}");
            } else {
                response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json");
                response.getWriter().write(
                        "{\"error\":\"Internal server error during"
                                + " payment verification\"}");
            }
            return null;
        }

        if (!vr.isValid) {
            respond402(response, request, config, vr.invalidReason);
            return null;
        }

        for (PaymentHooks.AfterVerifyHook hook : afterVerifyHooks) {
            hook.afterVerify(payload, requirements, vr);
        }

        return new VerifyResult(payload, requirements, config);
    }

    /**
     * Post-handler phase: settle (sync or async).
     * Only called when handler returned success (status &lt; 400).
     */
    public void postHandle(VerifyResult result,
                           HttpServletRequest request,
                           HttpServletResponse response) throws IOException {
        if (result == null || !result.isVerified()
                || response.getStatus() >= 400) {
            return;
        }

        if (result.config.asyncSettle) {
            if (settleExecutor == null) {
                throw new IllegalStateException(
                        "RouteConfig.asyncSettle=true but no settleExecutor"
                                + " configured. Call settleExecutor(executor)"
                                + " to provide one.");
            }
            response.setHeader(HEADER_SETTLEMENT_STATUS, "pending");
            response.setHeader("Access-Control-Expose-Headers",
                    HEADER_PAYMENT_RESPONSE + ", " + HEADER_SETTLEMENT_STATUS);

            settleExecutor.execute(() ->
                    executeAsyncSettle(result.payload, result.requirements,
                            result.config.syncSettle));
        } else {
            executeSyncSettle(result.payload, result.requirements,
                    result.config, request, response);
        }
    }

    // -----------------------------------------------------------------------
    // Settlement logic
    // -----------------------------------------------------------------------

    private void executeSyncSettle(PaymentPayload payload,
                                   PaymentRequirements requirements,
                                   RouteConfig config,
                                   HttpServletRequest request,
                                   HttpServletResponse response)
            throws IOException {

        for (PaymentHooks.BeforeSettleHook hook : beforeSettleHooks) {
            PaymentHooks.AbortResult ar = hook.beforeSettle(payload, requirements);
            if (ar != null && ar.abort) {
                if (!response.isCommitted()) {
                    respond402(response, request, config, ar.reason);
                }
                return;
            }
        }

        try {
            SettleResponse sr = facilitator.settle(
                    payload, requirements, config.syncSettle);

            if (!sr.success) {
                for (PaymentHooks.OnSettleFailureHook hook : settleFailureHooks) {
                    PaymentHooks.RecoverResult<SettleResponse> rr =
                            hook.onSettleFailure(payload, requirements, null);
                    if (rr != null && rr.recovered && rr.result != null) {
                        sr = rr.result;
                        break;
                    }
                }
                if (!sr.success) {
                    if (!response.isCommitted()) {
                        respond402(response, request, config,
                                sr.errorReason != null
                                        ? sr.errorReason : "settlement failed");
                    }
                    return;
                }
            }

            if ("timeout".equals(sr.status) && sr.transaction != null
                    && !sr.transaction.isEmpty()) {
                sr = recoverFromTimeout(sr.transaction, requirements.network);
            }

            if (!sr.success) {
                if (!response.isCommitted()) {
                    respond402(response, request, config,
                            sr.errorReason != null
                                    ? sr.errorReason : "settlement timeout");
                }
                return;
            }

            for (PaymentHooks.AfterSettleHook hook : afterSettleHooks) {
                hook.afterSettle(payload, requirements, sr);
            }

            SettlementResponseHeader srh = new SettlementResponseHeader(
                    true, sr.transaction, sr.network, sr.payer);
            String b64 = Base64.getEncoder().encodeToString(
                    Json.MAPPER.writeValueAsBytes(srh));
            response.setHeader(HEADER_PAYMENT_RESPONSE, b64);
            response.setHeader("Access-Control-Expose-Headers",
                    HEADER_PAYMENT_RESPONSE);
        } catch (Exception e) {
            if (!response.isCommitted()) {
                respond402(response, request, config,
                        "settlement error: " + e.getMessage());
            }
        }
    }

    private void executeAsyncSettle(PaymentPayload payload,
                                    PaymentRequirements requirements,
                                    boolean syncSettle) {
        try {
            for (PaymentHooks.BeforeSettleHook hook : beforeSettleHooks) {
                PaymentHooks.AbortResult ar =
                        hook.beforeSettle(payload, requirements);
                if (ar != null && ar.abort) {
                    if (asyncSettleCallback != null) {
                        SettleResponse aborted = new SettleResponse();
                        aborted.success = false;
                        aborted.errorReason = ar.reason;
                        asyncSettleCallback.onResult(
                                payload, requirements, aborted, null);
                    }
                    return;
                }
            }

            SettleResponse sr = facilitator.settle(
                    payload, requirements, syncSettle);

            if (!sr.success) {
                for (PaymentHooks.OnSettleFailureHook hook : settleFailureHooks) {
                    PaymentHooks.RecoverResult<SettleResponse> rr =
                            hook.onSettleFailure(payload, requirements, null);
                    if (rr != null && rr.recovered && rr.result != null) {
                        sr = rr.result;
                        break;
                    }
                }
            }

            if ("timeout".equals(sr.status) && sr.transaction != null
                    && !sr.transaction.isEmpty()) {
                sr = recoverFromTimeout(sr.transaction, requirements.network);
            }

            if (sr.success) {
                for (PaymentHooks.AfterSettleHook hook : afterSettleHooks) {
                    hook.afterSettle(payload, requirements, sr);
                }
            }

            if (asyncSettleCallback != null) {
                asyncSettleCallback.onResult(payload, requirements, sr, null);
            }
        } catch (Exception e) {
            if (asyncSettleCallback != null) {
                asyncSettleCallback.onResult(payload, requirements, null, e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    boolean isPaymentRequired(HttpServletRequest request) {
        if (Boolean.FALSE.equals(
                request.getAttribute(PAYMENT_REQUIRED_ATTRIBUTE))) {
            return false;
        }
        if (paymentRequiredPredicate != null) {
            return paymentRequiredPredicate.isRequired(request);
        }
        return true;
    }

    RouteConfig matchRoute(HttpServletRequest request) {
        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            config = routes.get(request.getRequestURI());
        }
        return config;
    }

    private SettleResponse recoverFromTimeout(String txHash, String network) {
        SettleResponse polled = pollSettleStatus(txHash);
        if (polled != null && polled.success
                && "success".equals(polled.status)) {
            return polled;
        }
        if (polled != null && !polled.success
                && "failed".equals(polled.status)) {
            return polled;
        }

        if (timeoutHook != null && timeoutHook.onTimeout(txHash, network)) {
            SettleResponse recovered = new SettleResponse();
            recovered.success = true;
            recovered.transaction = txHash;
            recovered.network = network;
            recovered.status = "success";
            return recovered;
        }

        SettleResponse denied = new SettleResponse();
        denied.success = false;
        denied.errorReason = "settlement_timeout";
        denied.errorMessage = "Settlement timed out and was not confirmed";
        denied.transaction = txHash;
        denied.network = network;
        return denied;
    }

    private SettleResponse pollSettleStatus(String txHash) {
        long deadlineMs = System.currentTimeMillis() + pollDeadline.toMillis();
        long intervalMs = pollInterval.toMillis();
        SettleResponse last = null;

        while (System.currentTimeMillis() < deadlineMs) {
            try {
                last = facilitator.settleStatus(txHash);
                if (last.success && "success".equals(last.status)) {
                    return last;
                }
                if (!last.success || "failed".equals(last.status)) {
                    return last;
                }
            } catch (Exception e) {
                // retry within deadline
            }
            long remaining = deadlineMs - System.currentTimeMillis();
            if (remaining <= 0) {
                break;
            }
            try {
                Thread.sleep(Math.min(intervalMs, remaining));
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                break;
            }
        }
        return last;
    }

    PaymentRequirements buildRequirements(HttpServletRequest request,
                                          RouteConfig config) {
        String price = config.priceFunction != null
                ? config.priceFunction.resolve(request) : config.price;
        ResolvedPrice resolved =
                AssetRegistry.resolvePrice(price, config.network);

        PaymentRequirements pr = new PaymentRequirements();
        pr.scheme = config.scheme;
        pr.network = config.network;
        pr.amount = resolved.amount();
        pr.payTo = config.payTo;
        pr.maxTimeoutSeconds = config.maxTimeoutSeconds;
        pr.asset = config.asset != null ? config.asset : resolved.asset();
        pr.extra = resolved.extra();
        return pr;
    }

    void respond402(HttpServletResponse resp, HttpServletRequest req,
                    RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL().toString();
        pr.resource.mimeType = "application/json";
        pr.accepts = List.of(buildRequirements(req, config));

        String json = Json.MAPPER.writeValueAsString(pr);
        String b64 = Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
        resp.setHeader("PAYMENT-REQUIRED", b64);
        resp.setHeader("Access-Control-Expose-Headers", "PAYMENT-REQUIRED");
        resp.setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json");
        resp.getWriter().write(json);
    }
}
