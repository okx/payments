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

import com.okx.x402.subscription.server.SubscriptionSchemeHandler;

import java.io.IOException;
import java.lang.System.Logger;
import java.lang.System.Logger.Level;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
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
    static final String HEADER_APP_PAYMENT = "X-APP-PAYMENT";
    static final String HEADER_APP_PAYMENT_REQUIRED = "X-APP-PAYMENT-REQUIRED";

    /**
     * Canonical subscription scheme identifier (matches the facilitator's /supported broadcast).
     */
    public static final String SCHEME_PERIOD = "period";
    /** Legacy subscription scheme alias (earlier releases); still recognized inbound. */
    public static final String SCHEME_PERMIT2_SUBSCRIPTION = "permit2_subscription";

    /**
     * Subscription lifecycle operations a route can be dedicated to:
     * CHANGE serves plan-change offers on an APP-Access proof
     * and executes a signed change; CANCEL / CANCEL_PENDING_CHANGE relay a buyer-signed
     * cancel auth to the facilitator (no 402 flow — a signed-auth relay, not a payment).
     */
    public enum SubscriptionOperation { CHANGE, CANCEL, CANCEL_PENDING_CHANGE }

    public static final Duration DEFAULT_POLL_INTERVAL = Duration.ofSeconds(1);
    public static final Duration DEFAULT_POLL_DEADLINE = Duration.ofSeconds(5);

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;
    /** Lower-cased payer addresses exempt from payment (empty = disabled). */
    private final Set<String> exemptPayers = new LinkedHashSet<>();
    private Duration pollInterval = DEFAULT_POLL_INTERVAL;
    private Duration pollDeadline = DEFAULT_POLL_DEADLINE;
    private PaymentHooks.OnSettlementTimeoutHook settlementTimeoutHook;
    private SubscriptionSchemeHandler subscriptionHandler;
    private final List<PaymentHooks.OnProtectedRequestHook> protectedRequestHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeVerifyHook> beforeVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterVerifyHook> afterVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.OnVerifyFailureHook> verifyFailureHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeSettleHook> beforeSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterSettleHook> afterSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.OnSettleFailureHook> settleFailureHooks = new ArrayList<>();

    private Executor settleExecutor;
    private AsyncSettleCallback asyncSettleCallback;
    /** Lazily-fetched /supported broadcast for subscription offer enhancement. */
    private volatile com.okx.x402.model.v2.SupportedResponse supportedCache;

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

        /**
         * Route-level settlement flags (apply to every option for this route).
         *
         * <p>{@code syncSettle} is tri-state: null = unset. Unset means {@code false} for the
         * exact/upto settle path (legacy behaviour) and {@code true} for the subscription
         * settle path.
         */
        public Boolean syncSettle;
        public boolean asyncSettle;

        /**
         * Marks this route as a dedicated subscription lifecycle operation (change / cancel /
         * cancel-pending-change), dispatched to the {@link SubscriptionSchemeHandler} before the
         * normal payment flow. Null for ordinary protected-resource routes.
         */
        public SubscriptionOperation subscriptionOperation;

        /**
         * Accepted public host names for resource-URL validation. When non-null
         * and non-empty, {@code payload.resource.url}'s host must be one of these
         * and its path must equal the request path — the request URL's own host
         * is not checked, so reverse proxy / CDN / internal-gateway host rewrites
         * no longer cause a false {@code "resource mismatch"}.
         *
         * <p>When null or empty, validation falls back to strict full-URL
         * equality (legacy behaviour), so existing integrations are unaffected.
         *
         * <p>This is still an intent check: the seller explicitly declares the
         * set of legal public domains so a buyer can't be tricked into signing a
         * payment bound to a third-party host. Path is compared explicitly.
         */
        public List<String> acceptedDomains;

        /**
         * Returns the effective list of accept options for this route. If
         * {@link #accepts} is populated, it is returned verbatim with missing
         * per-option fields defaulted from the route. Otherwise a one-element
         * list is synthesized from the legacy single-option fields.
         *
         * <p>Public so the subscription handler binds terms against the same
         * defaulted options the 402 envelope advertises (route-level
         * payTo/asset backfill must apply on both paths).
         */
        public List<AcceptOption> effectiveAccepts() {
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

        /**
         * True when the payer is on the exempt-payer whitelist and was
         * authenticated via the facilitator's signature-only endpoint.
         * The resource is served free of charge: settlement is skipped and
         * no PAYMENT-RESPONSE header is written.
         */
        public final boolean exempt;

        VerifyResult(PaymentPayload payload, PaymentRequirements requirements,
                     RouteConfig config) {
            this(payload, requirements, config, false);
        }

        VerifyResult(PaymentPayload payload, PaymentRequirements requirements,
                     RouteConfig config, boolean exempt) {
            this.payload = payload;
            this.requirements = requirements;
            this.config = config;
            this.exempt = exempt;
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

    /**
     * Configures the exempt-payer whitelist (merchant-controlled). When
     * non-empty, a request whose decoded {@code authorization.from} /
     * {@code permit2Authorization.from} is on this list is authenticated via
     * the facilitator's signature-only endpoint (no balance / nonce check);
     * on success the resource is served with HTTP 200 and both verify and
     * settle are skipped — no funds move.
     *
     * <p>This is a general SDK capability: merchants may whitelist their own
     * test wallets or free-tier customers. It is also REQUIRED for OKX.AI
     * marketplace review — configure the OKX review test wallet address here,
     * otherwise the automated review (which calls the endpoint with the test
     * wallet and expects 200 + deliverable without settlement) will reject
     * the service.
     *
     * <p>Security: whitelist membership is decided by the payload's
     * {@code from} field locally, but access is only granted after the
     * facilitator attests {@code isValid=true}, which guarantees signer ==
     * from (needs the wallet's private key / session key — a regular user
     * cannot forge it). The signed authorization is bound to this quote
     * (payTo + network), so it cannot be replayed at another merchant.
     * If signature verification fails or the endpoint is unavailable, the
     * request falls through to the normal verify → settle flow.
     *
     * <p>Addresses are matched case-insensitively. Passing an empty
     * collection disables the feature (default).
     *
     * @param payers payer wallet addresses to exempt; replaces any
     *               previously configured list
     * @return this processor for chaining
     */
    public PaymentProcessor exemptPayers(Collection<String> payers) {
        Objects.requireNonNull(payers, "payers");
        exemptPayers.clear();
        for (String p : payers) {
            if (p != null && !p.isBlank()) {
                exemptPayers.add(p.trim().toLowerCase(Locale.ROOT));
            }
        }
        return this;
    }

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

    public PaymentProcessor subscriptionHandler(SubscriptionSchemeHandler handler) {
        this.subscriptionHandler = Objects.requireNonNull(handler);
        // Change-route degradation: when the APP-Access proof on a CHANGE route
        // is invalid / unknown / unpaid, the handler falls back to writing the plain subscribe
        // offers (not a bare error) via this writer.
        handler.subscribeOfferWriter((req, resp, cfg, err) ->
                respond402Subscription(resp, req, cfg, err));
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

        RouteConfig config = matchRoute(request);
        if (config == null) {
            return VerifyResult.PASS_THROUGH;
        }

        // Dedicated subscription cancel routes relay a buyer-signed auth (no 402 flow) and
        // stay BEFORE the merchant hooks. The CHANGE operation is dispatched AFTER the
        // hook loop below, so merchant policy (bans, allowlists, free passes) applies to
        // change-offer requests exactly as it does to any other protected request.
        if (config.subscriptionOperation != null && subscriptionHandler != null
                && config.subscriptionOperation != SubscriptionOperation.CHANGE) {
            subscriptionHandler.handleCancelOperation(
                    request, response, config.subscriptionOperation);
            return null;
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
            // ABORT. Subscription routes answer 402; whether the offers envelope is attached
            // depends on the abort class: NOT_ELIGIBLE (period unpaid /
            // plan mismatch — paying fixes it) carries PAYMENT-REQUIRED; UNAUTHORIZED / DENIED /
            // PAYMENT_FAILED answer a bare 402 so an agent buyer is never told to (re)pay when
            // payment would not help. Non-subscription routes keep the plain 403.
            if (isSubscriptionScheme(config)) {
                respondSubscriptionAbort(response, request, config, pr);
            } else {
                respond403(response, pr.reason);
            }
            return null;
        }

        // CHANGE operation route: serve plan-change offers on APP-Access; a payment header
        // falls through to the normal subscribe/change path below. Change offers flow
        // through the SAME /supported enhancement as subscribe-402s — without
        // contracts/domain a buyer cannot sign the change, and cold-start behavior must not
        // depend on whether a subscribe-402 happened to run first on this route.
        if (config.subscriptionOperation == SubscriptionOperation.CHANGE
                && subscriptionHandler != null) {
            RouteConfig enhanced = withEnhancedSubscriptionOptions(config);
            if (subscriptionHandler.handleChangeOffers(request, response, enhanced)) {
                return null; // offers written (402) or offer-phase error
            }
        }

        // PRE-settle path: subscription scheme handles verify+settle in one step. On a MIXED
        // route (subscription + non-subscription accepts) the branch is chosen by the
        // PAYMENT'S scheme, not the route's: an
        // exact payment on a "buy or subscribe" menu falls through to the generic
        // verify/settle path instead of being rejected by the subscription handler.
        if (isSubscriptionScheme(config) && subscriptionHandler != null
                && shouldDelegateToSubscription(request, config)) {
            PaymentHooks.ProtectedRequestResult result;
            try {
                result = subscriptionHandler.handle(request, response, config);
            } catch (IOException e) {
                // ANY subscription settle failure —
                // including a facilitator network failure — answers a bare 402 (agent buyers
                // retry via the payment flow, not a gateway retry); 502 is reserved for the
                // cancel operation routes. Envelope-level errors
                // arrive as SubscriptionException and are 402-classified inside the handler.
                LOG.log(Level.WARNING, "subscription settle failed: facilitator unreachable", e);
                respond402Bare(response, "subscription settle failed: facilitator unreachable");
                return null;
            }
            if (result == null) {
                respond402Subscription(response, request, config, null);
                return null;
            }
            if (result.decision == PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS) {
                return VerifyResult.PASS_THROUGH;
            }
            if (result.decision == PaymentHooks.ProtectedRequestResult.Decision.ABORT) {
                respondSubscriptionAbort(response, request, config, result);
                return null;
            }
        }

        String header = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (header == null || header.isEmpty()) {
            header = request.getHeader(HEADER_X_PAYMENT);
        }
        if (header == null || header.isEmpty()) {
            // For subscription routes without a handler, check X-APP-PAYMENT
            if (isSubscriptionScheme(config)) {
                header = request.getHeader(HEADER_APP_PAYMENT);
                if (header == null || header.isEmpty()) {
                    respond402Subscription(response, request, config, null);
                    return null;
                }
            } else {
                respond402(response, request, config, null);
                return null;
            }
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
            if (!resourceMatches(payload.resource.url, requestUrl, config.acceptedDomains)) {
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

        // Exempt-payer short-circuit: if the whitelist is enabled and the
        // payload's `from` is on it, authenticate via the facilitator's
        // signature-only endpoint (no balance / nonce check — the test wallet
        // holds zero balance so the full verify would fail, and aggr_deferred
        // session-key signatures cannot be recovered locally). isValid=true
        // guarantees signer == from, so serve the resource and skip both
        // verify and settle. Any failure falls through to the normal flow.
        if (!exemptPayers.isEmpty()) {
            String from = extractPayerFrom(payload);
            if (from != null
                    && exemptPayers.contains(from.toLowerCase(Locale.ROOT))) {
                try {
                    VerifyResponse sig =
                            facilitator.verifySignature(payload, requirements);
                    if (sig != null && sig.isValid) {
                        return new VerifyResult(
                                payload, requirements, config, true);
                    }
                    LOG.log(Level.INFO,
                            "[x402] exempt payer signature invalid;"
                                    + " falling back to normal flow");
                } catch (Exception e) {
                    LOG.log(Level.WARNING,
                            "[x402] exempt-payer signature verification"
                                    + " failed; falling back to normal flow", e);
                }
            }
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
            // Log the full cause (which may include facilitator/path detail)
            // server-side only; never echo it into the client-facing body.
            LOG.log(Level.WARNING, "payment verification failed", e);
            response.setStatus(X402Response.SC_INTERNAL_SERVER_ERROR);
            response.setContentType("application/json; charset=UTF-8");
            response.writeBody(
                    "{\"error\":\"Internal server error during"
                            + " payment verification\"}");
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

        // Exempt payer: resource already served free of charge — no
        // settlement, no funds move, no PAYMENT-RESPONSE header.
        if (result.exempt) {
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
                            Boolean.TRUE.equals(result.config.syncSettle)));
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
                    payload, requirements, Boolean.TRUE.equals(config.syncSettle));

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
            // Detail may carry facilitator response body / internal path —
            // log it server-side and return a generic reason to the client.
            LOG.log(Level.WARNING, "settlement failed", e);
            if (!response.isCommitted()) {
                respond402(response, request, config, "settlement error");
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

    /**
     * Extracts the claimed payer address from a decoded payment payload.
     * Source by scheme: exact + EIP-3009 and aggr_deferred →
     * {@code payload.authorization.from}; exact + Permit2 / upto →
     * {@code payload.permit2Authorization.from}. Returns null when neither
     * shape is present (unknown scheme) — membership cannot be confirmed, so
     * the caller must NOT exempt and falls through to the normal flow.
     *
     * <p>The returned value is the client's claim only; it is trusted solely
     * after the facilitator's signature-only endpoint attests that the
     * payload signature was produced by this address.
     */
    static String extractPayerFrom(PaymentPayload payload) {
        if (payload == null || payload.payload == null) {
            return null;
        }
        String from = fromField(payload.payload.get("authorization"));
        if (from != null) {
            return from;
        }
        return fromField(payload.payload.get("permit2Authorization"));
    }

    private static String fromField(Object authorization) {
        if (authorization instanceof Map<?, ?> m) {
            Object from = m.get("from");
            if (from instanceof String s && !s.isBlank()) {
                return s;
            }
        }
        return null;
    }

    /**
     * Route lookup on the NORMALIZED request path: strips
     * query/fragment, collapses duplicate slashes, and drops a trailing slash so URI variants
     * ({@code /premium/}, {@code //premium}, {@code /premium?x=1}) cannot slip past an
     * exact-string route match while the downstream framework still serves the resource
     * (unpaid-bypass guard). Raw-URI lookup is kept as a fallback for routes registered
     * with a non-normalized key.
     */
    RouteConfig matchRoute(X402Request request) {
        String rawUri = request.getRequestURI();
        String path = normalizePath(rawUri);
        RouteConfig config = routes.get(request.getMethod() + " " + path);
        if (config == null) {
            config = routes.get(path);
        }
        if (config == null && !path.equals(rawUri)) {
            config = routes.get(request.getMethod() + " " + rawUri);
            if (config == null) {
                config = routes.get(rawUri);
            }
        }
        return config;
    }

    /** Strip query/fragment, collapse {@code //}, drop trailing slash (never below "/"). */
    static String normalizePath(String uri) {
        if (uri == null || uri.isEmpty()) {
            return "/";
        }
        String p = uri;
        int q = p.indexOf('?');
        if (q >= 0) {
            p = p.substring(0, q);
        }
        int f = p.indexOf('#');
        if (f >= 0) {
            p = p.substring(0, f);
        }
        StringBuilder sb = new StringBuilder(p.length());
        char prev = 0;
        for (int i = 0; i < p.length(); i++) {
            char c = p.charAt(i);
            if (c == '/' && prev == '/') {
                continue;
            }
            sb.append(c);
            prev = c;
        }
        p = sb.toString();
        if (p.length() > 1 && p.endsWith("/")) {
            p = p.substring(0, p.length() - 1);
        }
        return p.isEmpty() ? "/" : p;
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

        List<PaymentRequirements> offered;
        try {
            offered = buildRequirementsList(request, config);
        } catch (IllegalStateException e) {
            respond500RequirementsFailed(response);
            return null;
        }
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
     * Validates the buyer-signed {@code payload.resource.url} against the actual
     * request URL.
     *
     * <p>When {@code acceptedDomains} is null or empty, requires strict full-URL
     * equality (legacy behaviour). Otherwise the payload URL's host must be in
     * {@code acceptedDomains} and its path must equal the request path; the
     * request URL's host is intentionally not checked, so reverse proxy / CDN
     * host rewrites do not cause a false mismatch.
     */
    private static boolean resourceMatches(String payloadUrl, String requestUrl,
                                           List<String> acceptedDomains) {
        if (acceptedDomains == null || acceptedDomains.isEmpty()) {
            return Objects.equals(payloadUrl, requestUrl);   // backwards compatible
        }
        URI payloadUri = safeParse(payloadUrl);
        URI requestUri = safeParse(requestUrl);
        if (payloadUri == null || requestUri == null) {
            return false;
        }
        String payloadHost = payloadUri.getHost();
        if (payloadHost == null || !acceptedDomains.contains(payloadHost)) {
            return false;
        }
        return Objects.equals(payloadUri.getPath(), requestUri.getPath());
    }

    private static URI safeParse(String url) {
        if (url == null) {
            return null;
        }
        try {
            return URI.create(url);
        } catch (IllegalArgumentException e) {
            return null;
        }
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
            try {
                out.add(buildRequirement(request, opt));
            } catch (RuntimeException e) {
                // One mis-configured option (unregistered asset, bad price) is
                // warn-and-skip — it must not turn the whole route's 402 into a 500.
                LOG.log(Level.WARNING, "failed to build payment requirements for option "
                        + opt.scheme + "/" + opt.network, e);
            }
        }
        if (out.isEmpty()) {
            throw new IllegalStateException("failed to build payment requirements");
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

    /**
     * Backfill subscription options from the facilitator's {@code /supported} broadcast
     * ({@link com.okx.x402.subscription.server.SubscriptionOfferEnhancer} never overwrites
     * seller-pinned keys). Operates on a PER-REQUEST COPY of the route config: the seller's
     * shared {@link RouteConfig} is never mutated, so concurrent first requests can no longer
     * tear each other's {@code extra} maps (CME) while one of them is still enhancing. A
     * subscription option still missing contracts/domain afterwards is warned loudly: buyers
     * cannot sign such an offer.
     *
     * @return an enhanced copy when the route has subscription options; {@code config}
     *         unchanged otherwise
     */
    private RouteConfig withEnhancedSubscriptionOptions(RouteConfig config) {
        if (config == null || config.accepts == null || config.accepts.isEmpty()
                || !isSubscriptionScheme(config)) {
            return config;
        }
        RouteConfig copy = copyForEnhancement(config);
        try {
            if (supportedCache == null) {
                supportedCache = facilitator.supported();
            }
            com.okx.x402.subscription.server.SubscriptionOfferEnhancer.enhance(
                    copy.accepts, supportedCache);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } catch (Exception e) {
            LOG.log(Level.WARNING,
                    "failed to enhance subscription offers from /supported", e);
        }
        for (AcceptOption opt : copy.accepts) {
            if (isSubscriptionSchemeName(opt.scheme) && (opt.extra == null
                    || !opt.extra.containsKey("contracts") || !opt.extra.containsKey("domain"))) {
                LOG.log(Level.WARNING, "subscription offer for network " + opt.network
                        + " is missing contracts/domain — buyers cannot sign it; check the"
                        + " facilitator /supported response or pin extra manually");
            }
        }
        return copy;
    }

    /**
     * Copy for {@link #withEnhancedSubscriptionOptions}: route fields are carried over,
     * each {@link AcceptOption} is duplicated and its {@code extra} map shallow-copied.
     * Shallow is sufficient — the enhancer only puts NEW top-level keys
     * (facilitator / contracts / domain); nested maps already present (plan, initialCharge)
     * are read-only on every consumer path.
     */
    private static RouteConfig copyForEnhancement(RouteConfig config) {
        RouteConfig copy = new RouteConfig();
        copy.scheme = config.scheme;
        copy.network = config.network;
        copy.payTo = config.payTo;
        copy.price = config.price;
        copy.asset = config.asset;
        copy.maxTimeoutSeconds = config.maxTimeoutSeconds;
        copy.priceFunction = config.priceFunction;
        copy.syncSettle = config.syncSettle;
        copy.asyncSettle = config.asyncSettle;
        copy.subscriptionOperation = config.subscriptionOperation;
        copy.acceptedDomains = config.acceptedDomains;
        List<AcceptOption> options = new ArrayList<>(config.accepts.size());
        for (AcceptOption src : config.accepts) {
            AcceptOption opt = new AcceptOption();
            opt.scheme = src.scheme;
            opt.network = src.network;
            opt.payTo = src.payTo;
            opt.price = src.price;
            opt.priceFunction = src.priceFunction;
            opt.asset = src.asset;
            opt.maxTimeoutSeconds = src.maxTimeoutSeconds;
            opt.extra = src.extra != null ? new LinkedHashMap<>(src.extra) : null;
            options.add(opt);
        }
        copy.accepts = options;
        return copy;
    }

    /** Clean 500 for a route whose EVERY accept option failed to build. */
    private void respond500RequirementsFailed(X402Response resp) throws IOException {
        resp.setStatus(X402Response.SC_INTERNAL_SERVER_ERROR);
        resp.setContentType("application/json; charset=UTF-8");
        resp.writeBody("{\"error\":\"failed to build payment requirements\"}");
    }

    void respond402(X402Response resp, X402Request req,
                    RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL();
        pr.resource.mimeType = "application/json";
        try {
            pr.accepts = buildRequirementsList(req, config);
        } catch (IllegalStateException e) {
            respond500RequirementsFailed(resp);
            return;
        }

        String json = Json.MAPPER.writeValueAsString(pr);
        String b64 = Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
        resp.setHeader("PAYMENT-REQUIRED", b64);
        resp.setHeader("Access-Control-Expose-Headers", "PAYMENT-REQUIRED");
        resp.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json; charset=UTF-8");
        resp.writeBody(json);
    }

    /**
     * Dispatch a subscription-route ABORT by its class: NOT_ELIGIBLE → 402 + offers;
     * everything else → bare 402 without the PAYMENT-REQUIRED envelope.
     */
    private void respondSubscriptionAbort(X402Response resp, X402Request req, RouteConfig config,
                                          PaymentHooks.ProtectedRequestResult result)
            throws IOException {
        if (result.abortClass == null
                || result.abortClass == PaymentHooks.ProtectedRequestResult.AbortClass.NOT_ELIGIBLE) {
            respond402Subscription(resp, req, config, result.reason);
        } else {
            respond402Bare(resp, result.reason);
        }
    }

    /**
     * Bare 402: JSON error body only, NO PAYMENT-REQUIRED header.
     * Used for auth failures, merchant vetoes, and failed payment attempts, where re-paying
     * is the wrong instruction for the buyer.
     */
    void respond402Bare(X402Response resp, String error) throws IOException {
        resp.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json; charset=UTF-8");
        resp.writeBody("{\"x402Version\":2,\"error\":"
                + Json.MAPPER.writeValueAsString(error == null ? "" : error) + "}");
    }

    void respond402Subscription(X402Response resp, X402Request req,
                                RouteConfig config, String error) throws IOException {
        // Backfill contracts / facilitator / EIP-712 domain from the facilitator's /supported
        // broadcast before advertising — without
        // them a buyer cannot sign. Seller-pinned values always win; failures only warn.
        // The enhanced view is a per-request copy; the seller's config is never mutated.
        config = withEnhancedSubscriptionOptions(config);

        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL();
        pr.resource.mimeType = "application/json";
        try {
            pr.accepts = buildRequirementsList(req, config);
        } catch (IllegalStateException e) {
            respond500RequirementsFailed(resp);
            return;
        }

        String json = Json.MAPPER.writeValueAsString(pr);
        String b64 = Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
        // Canonical header is the standard PAYMENT-REQUIRED; the legacy
        // X-APP-PAYMENT-REQUIRED alias is kept for buyers on earlier releases.
        resp.setHeader("PAYMENT-REQUIRED", b64);
        resp.setHeader(HEADER_APP_PAYMENT_REQUIRED, b64);
        resp.setHeader("Access-Control-Expose-Headers",
                "PAYMENT-REQUIRED," + HEADER_APP_PAYMENT_REQUIRED);
        resp.setStatus(X402Response.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json; charset=UTF-8");
        resp.writeBody(json);
    }

    public static boolean isSubscriptionSchemeName(String scheme) {
        return SCHEME_PERIOD.equals(scheme) || SCHEME_PERMIT2_SUBSCRIPTION.equals(scheme);
    }

    /**
     * Mixed-route dispatch: a subscription-only route always delegates (a non-subscription
     * payload is rejected there with a no-matching-requirement 402); a route that
     * also advertises non-subscription accepts peeks the payment header's scheme/shape so
     * exact payments reach the generic verify/settle path.
     */
    private boolean shouldDelegateToSubscription(X402Request request, RouteConfig config) {
        boolean mixed = false;
        for (AcceptOption opt : config.effectiveAccepts()) {
            if (!isSubscriptionSchemeName(opt.scheme)) {
                mixed = true;
                break;
            }
        }
        if (!mixed) {
            return true;
        }
        return com.okx.x402.subscription.server.SubscriptionSchemeHandler
                .looksLikeSubscriptionPayment(request);
    }

    private boolean isSubscriptionScheme(RouteConfig config) {
        if (isSubscriptionSchemeName(config.scheme)) {
            return true;
        }
        if (config.accepts != null) {
            for (AcceptOption opt : config.accepts) {
                if (isSubscriptionSchemeName(opt.scheme)) {
                    return true;
                }
            }
        }
        return false;
    }
}
