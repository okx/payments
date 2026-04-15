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

import jakarta.servlet.Filter;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.ServletRequest;
import jakarta.servlet.ServletResponse;
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

/**
 * Servlet filter that enforces x402 V2 payments on configured routes.
 * Supports route-level pricing, multi-chain, dynamic price resolution,
 * and settlement timeout polling with developer hook fallback.
 *
 * <p>Settlement timeout recovery flow (when settle returns status="timeout"):
 * <ol>
 *   <li>SDK auto-polls GET /settle/status (interval 1s, deadline configurable, default 5s)</li>
 *   <li>If status="success" during polling → return 200 + resource</li>
 *   <li>If polling expires and developer configured a timeout hook → call hook</li>
 *   <li>Hook returns confirmed=true → return 200; confirmed=false → return 402</li>
 *   <li>No hook configured → return 402</li>
 * </ol>
 */
public class PaymentFilter implements Filter {

    private static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    private static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    private static final String HEADER_X_PAYMENT = "X-PAYMENT";

    /** Default polling interval for settle/status checks. */
    public static final Duration DEFAULT_POLL_INTERVAL = Duration.ofSeconds(1);
    /** Default polling deadline before calling timeout hook. */
    public static final Duration DEFAULT_POLL_DEADLINE = Duration.ofSeconds(5);

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;
    private Duration pollInterval = DEFAULT_POLL_INTERVAL;
    private Duration pollDeadline = DEFAULT_POLL_DEADLINE;
    private OnSettlementTimeoutHook timeoutHook;

    // Lifecycle hooks
    private final List<PaymentHooks.BeforeVerifyHook> beforeVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterVerifyHook> afterVerifyHooks = new ArrayList<>();
    private final List<PaymentHooks.OnVerifyFailureHook> verifyFailureHooks = new ArrayList<>();
    private final List<PaymentHooks.BeforeSettleHook> beforeSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.AfterSettleHook> afterSettleHooks = new ArrayList<>();
    private final List<PaymentHooks.OnSettleFailureHook> settleFailureHooks = new ArrayList<>();

    /**
     * Dynamic pricing function. Resolves price per-request at runtime.
     * Matches Go's DynamicPriceFunc and TypeScript's DynamicPrice pattern.
     */
    @FunctionalInterface
    public interface DynamicPrice {
        /**
         * Resolve the price for the given request.
         *
         * @param request the HTTP request to price
         * @return price string (e.g. "$0.01" or atomic units)
         */
        String resolve(HttpServletRequest request);
    }

    /**
     * Hook called when settlement polling times out.
     * Developer decides whether to grant access (confirmed=true) or deny (confirmed=false).
     */
    @FunctionalInterface
    public interface OnSettlementTimeoutHook {
        /**
         * Called when settle status polling expires without success.
         *
         * @param txHash the transaction hash being polled
         * @param network the CAIP-2 network identifier
         * @return true to grant access (200), false to deny (402)
         */
        boolean onTimeout(String txHash, String network);
    }

    /**
     * Per-route payment configuration.
     */
    public static class RouteConfig {
        /** Payment scheme (default "exact"). */
        public String scheme = "exact";
        /** CAIP-2 network identifier (e.g. "eip155:196"). */
        public String network;
        /** Receiver wallet address. */
        public String payTo;
        /** Price as USD string (e.g. "$0.01") or atomic units. */
        public String price;
        /** Optional asset override (contract address). */
        public String asset;
        /** Maximum timeout in seconds (default 24h per spec). */
        public int maxTimeoutSeconds = 86400;
        /** Whether to use syncSettle (default false). */
        public boolean syncSettle;
        /** Optional dynamic pricing function (overrides static price if set). */
        public DynamicPrice priceFunction;
    }

    /**
     * Creates a PaymentFilter with the given facilitator and route configuration.
     *
     * @param facilitator V2 facilitator client
     * @param routes map of "METHOD /path" to route config
     */
    public PaymentFilter(FacilitatorClient facilitator, Map<String, RouteConfig> routes) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.routes = Objects.requireNonNull(routes);
    }

    /**
     * Convenience factory for one-line integration.
     *
     * @param facilitator V2 facilitator client
     * @param routes map of route keys to configs
     * @return configured PaymentFilter
     */
    public static PaymentFilter create(FacilitatorClient facilitator,
                                       Map<String, RouteConfig> routes) {
        return new PaymentFilter(facilitator, routes);
    }

    /**
     * Set the polling interval for settle/status checks.
     *
     * @param interval polling interval (default 1s)
     * @return this filter for chaining
     */
    public PaymentFilter pollInterval(Duration interval) {
        this.pollInterval = Objects.requireNonNull(interval);
        return this;
    }

    /**
     * Set the polling deadline before calling timeout hook.
     *
     * @param deadline max duration to poll (default 5s)
     * @return this filter for chaining
     */
    public PaymentFilter pollDeadline(Duration deadline) {
        this.pollDeadline = Objects.requireNonNull(deadline);
        return this;
    }

    /**
     * Register a settlement timeout hook. Called when settle status polling
     * expires without success. Developer decides whether to grant access.
     *
     * @param hook timeout hook (txHash, network) → confirmed
     * @return this filter for chaining
     */
    public PaymentFilter onSettlementTimeout(OnSettlementTimeoutHook hook) {
        this.timeoutHook = hook;
        return this;
    }

    /** Register a before-verify lifecycle hook. */
    public PaymentFilter onBeforeVerify(PaymentHooks.BeforeVerifyHook hook) {
        beforeVerifyHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Register an after-verify lifecycle hook. */
    public PaymentFilter onAfterVerify(PaymentHooks.AfterVerifyHook hook) {
        afterVerifyHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Register a verify-failure lifecycle hook. */
    public PaymentFilter onVerifyFailure(PaymentHooks.OnVerifyFailureHook hook) {
        verifyFailureHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Register a before-settle lifecycle hook. */
    public PaymentFilter onBeforeSettle(PaymentHooks.BeforeSettleHook hook) {
        beforeSettleHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Register an after-settle lifecycle hook. */
    public PaymentFilter onAfterSettle(PaymentHooks.AfterSettleHook hook) {
        afterSettleHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    /** Register a settle-failure lifecycle hook. */
    public PaymentFilter onSettleFailure(PaymentHooks.OnSettleFailureHook hook) {
        settleFailureHooks.add(Objects.requireNonNull(hook));
        return this;
    }

    @Override
    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException, ServletException {

        if (!(req instanceof HttpServletRequest request)
                || !(res instanceof HttpServletResponse response)) {
            chain.doFilter(req, res);
            return;
        }

        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            config = routes.get(request.getRequestURI());
        }
        if (config == null) {
            chain.doFilter(req, res);
            return;
        }

        // Read PAYMENT-SIGNATURE (V2) or X-PAYMENT (V1 fallback)
        String header = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (header == null || header.isEmpty()) {
            header = request.getHeader(HEADER_X_PAYMENT);
        }
        if (header == null || header.isEmpty()) {
            respond402(response, request, config, null);
            return;
        }

        // Decode payment payload
        PaymentPayload payload;
        try {
            payload = PaymentPayload.fromHeader(header);
        } catch (Exception e) {
            respond402(response, request, config, "malformed payment header");
            return;
        }

        // Validate resource URL matches (prevent cross-route replay)
        if (payload.resource != null && payload.resource.url != null) {
            String requestUrl = request.getRequestURL().toString();
            if (!payload.resource.url.equals(requestUrl)) {
                respond402(response, request, config, "resource mismatch");
                return;
            }
        }

        PaymentRequirements requirements = buildRequirements(request, config);

        // --- Before-verify hooks ---
        for (PaymentHooks.BeforeVerifyHook hook : beforeVerifyHooks) {
            PaymentHooks.AbortResult ar = hook.beforeVerify(payload, requirements);
            if (ar != null && ar.abort) {
                respond402(response, request, config, ar.reason);
                return;
            }
        }

        VerifyResponse vr;
        try {
            vr = facilitator.verify(payload, requirements);
        } catch (Exception e) {
            // --- Verify-failure hooks (can recover) ---
            for (PaymentHooks.OnVerifyFailureHook hook : verifyFailureHooks) {
                PaymentHooks.RecoverResult<VerifyResponse> rr =
                        hook.onVerifyFailure(payload, requirements, e);
                if (rr != null && rr.recovered && rr.result != null) {
                    vr = rr.result;
                    // recovered — skip the error handling below
                    if (!vr.isValid) {
                        respond402(response, request, config, vr.invalidReason);
                        return;
                    }
                    break;
                }
            }
            // If no hook recovered, return error
            if (e instanceof IOException) {
                response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json");
                response.getWriter().write(
                        "{\"error\":\"Payment verification failed: " + e.getMessage() + "\"}");
            } else {
                response.setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
                response.setContentType("application/json");
                response.getWriter().write(
                        "{\"error\":\"Internal server error during payment verification\"}");
            }
            return;
        }

        if (!vr.isValid) {
            respond402(response, request, config, vr.invalidReason);
            return;
        }

        // --- After-verify hooks ---
        for (PaymentHooks.AfterVerifyHook hook : afterVerifyHooks) {
            hook.afterVerify(payload, requirements, vr);
        }

        // Execute business logic
        chain.doFilter(req, res);

        if (response.getStatus() >= 400) {
            return;
        }

        // --- Before-settle hooks ---
        for (PaymentHooks.BeforeSettleHook hook : beforeSettleHooks) {
            PaymentHooks.AbortResult ar = hook.beforeSettle(payload, requirements);
            if (ar != null && ar.abort) {
                if (!response.isCommitted()) {
                    respond402(response, request, config, ar.reason);
                }
                return;
            }
        }

        // Settle (with timeout polling + hook recovery)
        try {
            SettleResponse sr = facilitator.settle(payload, requirements, config.syncSettle);

            if (!sr.success) {
                // --- Settle-failure hooks (can recover) ---
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
                                sr.errorReason != null ? sr.errorReason : "settlement failed");
                    }
                    return;
                }
            }

            // If status="timeout", attempt recovery via polling + hook
            if ("timeout".equals(sr.status) && sr.transaction != null
                    && !sr.transaction.isEmpty()) {
                sr = recoverFromTimeout(sr.transaction, requirements.network);
            }

            if (!sr.success) {
                if (!response.isCommitted()) {
                    respond402(response, request, config,
                            sr.errorReason != null ? sr.errorReason : "settlement timeout");
                }
                return;
            }

            // --- After-settle hooks ---
            for (PaymentHooks.AfterSettleHook hook : afterSettleHooks) {
                hook.afterSettle(payload, requirements, sr);
            }

            SettlementResponseHeader srh = new SettlementResponseHeader(
                    true, sr.transaction, sr.network, sr.payer);
            String b64 = Base64.getEncoder().encodeToString(
                    Json.MAPPER.writeValueAsBytes(srh));
            response.setHeader(HEADER_PAYMENT_RESPONSE, b64);
            response.setHeader("Access-Control-Expose-Headers", HEADER_PAYMENT_RESPONSE);
        } catch (Exception e) {
            if (!response.isCommitted()) {
                respond402(response, request, config, "settlement error: " + e.getMessage());
            }
        }
    }

    /**
     * Attempt to recover from a settlement timeout by polling settle/status
     * and falling back to the developer's timeout hook.
     */
    private SettleResponse recoverFromTimeout(String txHash, String network) {
        // Step 1: Poll settle/status
        SettleResponse polled = pollSettleStatus(txHash);
        if (polled != null && polled.success && "success".equals(polled.status)) {
            return polled;
        }
        if (polled != null && !polled.success && "failed".equals(polled.status)) {
            return polled;
        }

        // Step 2: Polling inconclusive — call timeout hook if configured
        if (timeoutHook != null) {
            boolean confirmed = timeoutHook.onTimeout(txHash, network);
            if (confirmed) {
                SettleResponse recovered = new SettleResponse();
                recovered.success = true;
                recovered.transaction = txHash;
                recovered.network = network;
                recovered.status = "success";
                return recovered;
            }
        }

        // Step 3: No hook or hook denied — return failure
        SettleResponse denied = new SettleResponse();
        denied.success = false;
        denied.errorReason = "settlement_timeout";
        denied.errorMessage = "Settlement timed out and was not confirmed";
        denied.transaction = txHash;
        denied.network = network;
        return denied;
    }

    /**
     * Poll GET /settle/status at regular intervals until success, failure, or deadline.
     *
     * @return the final SettleResponse, or null if all polls errored
     */
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
                // Log and retry within deadline
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

    private PaymentRequirements buildRequirements(HttpServletRequest request,
                                                  RouteConfig config) {
        String price = config.priceFunction != null
                ? config.priceFunction.resolve(request) : config.price;
        ResolvedPrice resolved = AssetRegistry.resolvePrice(price, config.network);

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

    private void respond402(HttpServletResponse resp, HttpServletRequest req,
                            RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL().toString();
        pr.resource.mimeType = "application/json";
        pr.accepts = List.of(buildRequirements(req, config));

        String json = Json.MAPPER.writeValueAsString(pr);

        // Set PAYMENT-REQUIRED header for cross-SDK compatibility (Go/TS expect it)
        String b64 = Base64.getEncoder().encodeToString(
                json.getBytes(StandardCharsets.UTF_8));
        resp.setHeader("PAYMENT-REQUIRED", b64);
        resp.setHeader("Access-Control-Expose-Headers", "PAYMENT-REQUIRED");

        resp.setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        resp.setContentType("application/json");
        resp.getWriter().write(json);
    }
}
