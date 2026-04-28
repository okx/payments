// SPDX-License-Identifier: Apache-2.0
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

import java.io.IOException;
import java.lang.System.Logger;
import java.lang.System.Logger.Level;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.Executor;

/**
 * Core payment processing logic shared by the Jakarta and Javax adapter
 * modules. Operates on the servlet-agnostic {@link X402Request} /
 * {@link X402Response} abstractions so the same logic works for both
 * namespaces.
 *
 * <p>Holds all configuration, hooks, and business logic. The filter and
 * interceptor implementations in the adapter modules are thin wrappers that
 * delegate to this class.
 */
public class PaymentProcessor {

    private static final Logger LOG =
            System.getLogger(PaymentProcessor.class.getName());

    static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    static final String HEADER_X_PAYMENT = "X-PAYMENT";
    static final String HEADER_SETTLEMENT_STATUS = "X-SETTLEMENT-STATUS";

    public static final Duration DEFAULT_POLL_INTERVAL = Duration.ofSeconds(1);
    public static final Duration DEFAULT_POLL_DEADLINE = Duration.ofSeconds(5);

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;
    private Duration pollInterval = DEFAULT_POLL_INTERVAL;
    private Duration pollDeadline = DEFAULT_POLL_DEADLINE;
    private PaymentHooks.OnSettlementTimeoutHook settlementTimeoutHook;
    private final List<PaymentHooks.OnProtectedRequestHook> protectedRequestHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeVerifyHook> beforeVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterVerifyHook> afterVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.OnVerifyFailureHook> verifyFailureHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeSettleHook> beforeSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterSettleHook> afterSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.OnSettleFailureHook> settleFailureHooks = new ArrayList<>();

    private Executor settleExecutor;
    private AsyncSettleCallback asyncSettleCallback;

    // -----------------------------------------------------------------------
    // Nested types
    // -----------------------------------------------------------------------

    @FunctionalInterface
    public interface DynamicPrice {
        String resolve(X402Request request);
    }

    @FunctionalInterface
    public interface AsyncSettleCallback {
        void onResult(PaymentPayload payload, PaymentRequirements requirements,
                      SettleResponse result, Throwable error);
    }

    public static class RouteConfig {
        /**
         * Single-option fields (legacy). Set these for the common case of "one
         * route charges one token with one scheme". Left unchanged for
         * backwards compatibility — existing integrations keep working.
         */
        public String scheme = "exact";
        public String network;
        public String payTo;
        public String price;
        public String asset;
        public int maxTimeoutSeconds = 86400;
        public DynamicPrice priceFunction;

        /**
         * Multi-option list. When non-null and non-empty, the 402
         * {@code accepts} envelope is built from this list (one
         * {@link PaymentRequirements} per element) — matching the Go/TS SDK
         * shape. Each {@link AcceptOption} may omit fields that are already
         * set at the route level (e.g. shared {@code network}, {@code payTo}).
         *
         * <p>When null or empty, the legacy single-option fields above are
         * used to synthesize a one-element list, preserving backwards
         * compatibility.
         */
        public List<AcceptOption> accepts;

        /** Route-level settlement flags (apply to every option for this route). */
        public boolean syncSettle;
        public boolean asyncSettle;

        /**
         * Returns the effective list of accept options for this route. If
         * {@link #accepts} is populated, it is returned verbatim with missing
         * per-option fields defaulted from the route. Otherwise a one-element
         * list is synthesized from the legacy single-option fields.
         */
        List<AcceptOption> effectiveAccepts() {
            if (accepts != null && !accepts.isEmpty()) {
                List<AcceptOption> resolved = new ArrayList<>(accepts.size());
                for (AcceptOption opt : accepts) {
                    resolved.add(withDefaultsFrom(opt, this));
                }
                return resolved;
            }
            AcceptOption legacy = new AcceptOption();
            legacy.scheme = this.scheme;
            legacy.network = this.network;
            legacy.payTo = this.payTo;
            legacy.price = this.price;
            legacy.priceFunction = this.priceFunction;
            legacy.asset = this.asset;
            legacy.maxTimeoutSeconds = this.maxTimeoutSeconds;
            return List.of(legacy);
        }

        private static AcceptOption withDefaultsFrom(AcceptOption src, RouteConfig route) {
            AcceptOption merged = new AcceptOption();
            merged.scheme = src.scheme != null ? src.scheme : route.scheme;
            merged.network = src.network != null ? src.network : route.network;
            merged.payTo = src.payTo != null ? src.payTo : route.payTo;
            merged.price = src.price != null ? src.price : route.price;
            merged.priceFunction = src.priceFunction != null
                    ? src.priceFunction : route.priceFunction;
            merged.asset = src.asset != null ? src.asset : route.asset;
            merged.maxTimeoutSeconds = src.maxTimeoutSeconds > 0
                    ? src.maxTimeoutSeconds : route.maxTimeoutSeconds;
            merged.extra = src.extra;
            return merged;
        }
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

    /**
     * Register the settlement-timeout hook. Single-hook: later registrations
     * replace earlier ones (matches TS semantics). Exceptions thrown by the
     * hook are caught and treated as {@code notConfirmed()}.
     */
    public PaymentProcessor onSettlementTimeout(
            PaymentHooks.OnSettlementTimeoutHook hook) {
        this.settlementTimeoutHook = hook;
        return this;
    }

    /**
     * Append an {@code onProtectedRequest} hook. Runs after route match and
     * before the payment header is read. The first hook returning
     * {@code GRANT_ACCESS} or {@code ABORT} wins; subsequent hooks are
     * skipped for that request.
     */
    public PaymentProcessor onProtectedRequest(
            PaymentHooks.OnProtectedRequestHook hook) {
        protectedRequestHooks.add(Objects.requireNonNull(hook));
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
    public VerifyResult preHandle(X402Request request, X402Response response)
            throws IOException {

        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            config = routes.get(request.getRequestURI());
        }
        if (config == null) {
            return VerifyResult.PASS_THROUGH;
        }

        for (PaymentHooks.OnProtectedRequestHook hook : protectedRequestHooks) {
            PaymentHooks.ProtectedRequestResult pr =
                    hook.onProtectedRequest(request, config);
            if (pr == null
                    || pr.decision == PaymentHooks.ProtectedRequestResult.Decision.PROCEED) {
                continue;
            }
            if (pr.decision == PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS) {
                return VerifyResult.PASS_THROUGH;
            }
            // ABORT — HTTP 403, not 402.
            respond403(response, pr.reason);
            return null;
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
            respond402(response, request, config, "invalid payment header");
            return null;
        }

        if (payload.resource != null && payload.resource.url != null) {
            String requestUrl = request.getRequestURL();
            if (!payload.resource.url.equals(requestUrl)) {
                respond402(response, request, config, "resource mismatch");
                return null;
            }
        }

        PaymentRequirements requirements =
                selectRequirementForPayload(request, config, payload, response);
        if (requirements == null) {
            // A 402 was already written by selectRequirementForPayload.
            return null;
        }

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
                response.setStatus(X402Response.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json; charset=UTF-8");
                response.writeBody(
                        "{\"error\":\"Payment verification failed: "
                                + e.getMessage() + "\"}");
            } else {
                response.setStatus(X402Response.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json; charset=UTF-8");
                response.writeBody(
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
                           X402Request request,
                           X402Response response) throws IOException {
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
                                   X402Request request,
                                   X402Response response)
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

    RouteConfig matchRoute(X402Request request) {
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

        if (settlementTimeoutHook != null) {
            boolean confirmed = false;
            try {
                PaymentHooks.SettlementTimeoutResult hr =
                        settlementTimeoutHook.onSettlementTimeout(txHash, network);
                confirmed = hr != null && hr.confirmed;
            } catch (Exception err) {
                LOG.log(Level.WARNING,
                        "[x402] onSettlementTimeout hook error", err);
            }
            if (confirmed) {
                SettleResponse recovered = new SettleResponse();
                recovered.success = true;
                recovered.transaction = txHash;
                recovered.network = network;
                recovered.status = "success";
                return recovered;
            }
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

    /**
     * Picks the canonical {@link PaymentRequirements} that matches what the
     * client selected in its {@code PaymentPayload.accepted}, or returns null
     * after writing a 402 if nothing matches. Matching is strict: the client
     * must echo the server's {@code (scheme, network, asset, payTo, amount)}
     * verbatim. This mirrors the TypeScript SDK's
     * {@code findMatchingRequirements} and prevents a tampered payload from
     * sneaking past the SDK and relying on the facilitator to catch it
     * (which is not a guarantee for all deployments).
     *
     * <p>{@code maxTimeoutSeconds} is compared only when the client supplies a
     * non-zero value; v1-shaped clients that omit it (Jackson-default 0) are
     * tolerated because the server-side requirement is authoritative.
     *
     * <p>{@code asset} and {@code payTo} are compared case-insensitively to
     * accommodate EIP-55 mixed-case checksumming.
     */
    private PaymentRequirements selectRequirementForPayload(
            X402Request request, RouteConfig config,
            PaymentPayload payload, X402Response response) throws IOException {

        List<PaymentRequirements> offered = buildRequirementsList(request, config);
        PaymentRequirements clientPick = payload.accepted;

        // Legacy payloads that omit `accepted` fall back to the first offered
        // option — keeps v1-style clients working against v2 servers.
        if (clientPick == null) {
            return offered.get(0);
        }

        // Strict match regardless of how many options the route offers. The
        // previous single-option short-circuit silently accepted any payload
        // for a one-option route, which masked client/server-schema mismatches
        // until the facilitator rejected them. Matches TS SDK's
        // `findMatchingRequirements` which always runs a strict match.
        for (PaymentRequirements r : offered) {
            if (matches(r, clientPick)) {
                return r;
            }
        }
        respond402(response, request, config, "no matching payment option");
        return null;
    }

    /**
     * Strict equality check between a server-offered {@link PaymentRequirements}
     * and the one echoed back in {@code PaymentPayload.accepted} by the client.
     *
     * <p>The amount field is the sole on-the-wire defence against price
     * tampering, so it is required: a client that omits {@code amount} (null)
     * is rejected even if every other field matches.
     */
    private static boolean matches(PaymentRequirements offered, PaymentRequirements picked) {
        if (!Objects.equals(offered.scheme, picked.scheme)) {
            return false;
        }
        if (!Objects.equals(offered.network, picked.network)) {
            return false;
        }
        if (!equalsIgnoreCaseOrEitherNull(offered.asset, picked.asset)) {
            return false;
        }
        if (!equalsIgnoreCaseOrEitherNull(offered.payTo, picked.payTo)) {
            return false;
        }
        // amount: client MUST echo server's value verbatim. A null on the
        // client side is treated as a mismatch — this is the only field that
        // protects the server from a malicious / buggy client signing a
        // smaller amount than what was offered.
        if (picked.amount == null
                || !Objects.equals(offered.amount, picked.amount)) {
            return false;
        }
        // maxTimeoutSeconds is a primitive int with default 0. Treat 0 as
        // "client did not set" and skip the comparison so v1-shaped clients
        // that omit the field continue to interoperate; the server's own
        // requirement value is authoritative on the verify/settle path.
        if (picked.maxTimeoutSeconds != 0
                && offered.maxTimeoutSeconds != picked.maxTimeoutSeconds) {
            return false;
        }
        return true;
    }

    private static boolean equalsIgnoreCaseOrEitherNull(String a, String b) {
        return a == null || b == null || a.equalsIgnoreCase(b);
    }

    /**
     * Builds the full list of {@link PaymentRequirements} the server offers for
     * this route — one per {@link AcceptOption} in {@code config.effectiveAccepts()}.
     * A route that only uses the legacy scalar fields returns a one-element list.
     */
    List<PaymentRequirements> buildRequirementsList(X402Request request,
                                                    RouteConfig config) {
        List<AcceptOption> options = config.effectiveAccepts();
        List<PaymentRequirements> out = new ArrayList<>(options.size());
        for (AcceptOption opt : options) {
            out.add(buildRequirement(request, opt));
        }
        return out;
    }

    /**
     * Legacy single-requirement builder. Retained for internal callers and
     * tests that only look at the "first" requirement for a route (e.g. the
     * {@code preHandle} verify path, which binds to the requirement selected
     * by the client's payment payload rather than re-issuing the 402).
     */
    PaymentRequirements buildRequirements(X402Request request,
                                          RouteConfig config) {
        List<AcceptOption> options = config.effectiveAccepts();
        return buildRequirement(request, options.get(0));
    }

    private PaymentRequirements buildRequirement(X402Request request,
                                                 AcceptOption opt) {
        String price = opt.priceFunction != null
                ? opt.priceFunction.resolve(request) : opt.price;
        ResolvedPrice resolved =
                AssetRegistry.resolvePrice(price, opt.network, opt.asset);

        PaymentRequirements pr = new PaymentRequirements();
        pr.scheme = opt.scheme;
        pr.network = opt.network;
        pr.amount = resolved.amount();
        pr.payTo = opt.payTo;
        pr.maxTimeoutSeconds = opt.maxTimeoutSeconds > 0
                ? opt.maxTimeoutSeconds : 86400;
        pr.asset = opt.asset != null ? opt.asset : resolved.asset();
        // Merge registry-resolved EIP-712 domain fields (name/version/transferMethod)
        // with caller-provided extras. Caller values win on key collision, but
        // registry fields are preserved for keys the caller didn't set — so
        // adding e.g. {"sessionCert": …} no longer wipes out name/version and
        // break signing. Matches TS SDK's `{ ...parsedPrice.extra, ...resourceConfig.extra }`.
        Map<String, Object> merged = new LinkedHashMap<>();
        if (resolved.extra() != null) {
            merged.putAll(resolved.extra());
        }
        if (opt.extra != null) {
            merged.putAll(opt.extra);
        }
        pr.extra = merged;
        return pr;
    }

    /**
     * Short HTTP 403 response used when {@link PaymentHooks.OnProtectedRequestHook}
     * returns {@code ABORT}. Body is {@code {"error":"<reason>"}}. Does NOT
     * write the {@code PAYMENT-REQUIRED} envelope — that is reserved for the
     * "payment required but missing/invalid" 402 path.
     */
    void respond403(X402Response resp, String reason) throws IOException {
        resp.setStatus(403);
        resp.setContentType("application/json; charset=UTF-8");
        String safeReason = reason == null ? "" : reason;
        String body = "{\"error\":"
                + Json.MAPPER.writeValueAsString(safeReason) + "}";
        resp.writeBody(body);
    }

    void respond402(X402Response resp, X402Request req,
                    RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL();
        pr.resource.mimeType = "application/json";
        pr.accepts = buildRequirementsList(req, config);

        String json = Json.MAPPER.writeValueAsString(pr);
        String b64 = Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
        resp.setHeader("PAYMENT-REQUIRED", b64);
        resp.setHeader("Access-Control-Expose-Headers", "PAYMENT-REQUIRED");
        resp.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json; charset=UTF-8");
        resp.writeBody(json);
    }
}
