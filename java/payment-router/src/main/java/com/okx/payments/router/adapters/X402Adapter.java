package com.okx.payments.router.adapters;

import com.okx.payments.router.ProtocolAdapter;
import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Request;
import com.okx.x402.server.X402Response;
import com.okx.x402.server.internal.JavaxRequestAdapter;

import javax.servlet.FilterChain;
import javax.servlet.ServletException;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;

/**
 * x402 {@link ProtocolAdapter} — wraps the javax {@code PaymentFilter} (and the
 * underlying {@code PaymentProcessor}) so that {@code PaymentRouterFilter} can
 * dispatch x402 alongside MPP on the same Servlet chain.
 *
 * <p>Mirrors {@link MppAdapter}'s shape:
 * <ul>
 *   <li>{@link #detect(HttpServletRequest)} — true when the request carries
 *       an x402 payment header ({@code X-PAYMENT} for v1 or
 *       {@code PAYMENT-SIGNATURE} for v2).</li>
 *   <li>{@link #getChallenge(HttpServletRequest, Object)} — drives
 *       {@code PaymentProcessor.preHandle} on a {@link CapturingX402Response}
 *       to produce the canonical {@code PAYMENT-REQUIRED} (base64-JSON) header
 *       for this route. {@code PaymentRouterFilter}'s ChallengeMerger then
 *       merges this with peers (e.g. MPP's {@code WWW-Authenticate: Payment})
 *       so a single 402 advertises both protocols.</li>
 *   <li>{@link #handle(HttpServletRequest, HttpServletResponse, FilterChain, Object)}
 *       — delegates to the embedded {@link PaymentFilter} so the verify +
 *       chain + settle pipeline runs unchanged.</li>
 * </ul>
 *
 * <p>Construction takes the same shape as {@link PaymentFilter#create}:
 * a facilitator client + a path-keyed route table. The route table is the
 * single source of truth for which paths require x402 payment; the
 * {@code Object cfg} parameter passed by {@code PaymentRouterFilter} is
 * informational only (we use the request URI for route lookup).
 */
public final class X402Adapter implements ProtocolAdapter {

    public static final int    DEFAULT_PRIORITY     = 20;
    public static final String NAME                 = "x402";

    /** x402 v1 / v2 request-side payment headers — presence triggers detect(). */
    private static final String HEADER_X_PAYMENT          = "X-PAYMENT";
    private static final String HEADER_PAYMENT_SIGNATURE  = "PAYMENT-SIGNATURE";
    /** v2 challenge header emitted by the underlying PaymentProcessor. */
    private static final String HEADER_PAYMENT_REQUIRED   = "PAYMENT-REQUIRED";
    private static final String HEADER_CORS_EXPOSE        = "Access-Control-Expose-Headers";

    private final PaymentProcessor processor;
    private final PaymentFilter delegateFilter;
    private final int priority;

    public X402Adapter(FacilitatorClient facilitator,
                       Map<String, PaymentProcessor.RouteConfig> routes) {
        this(new PaymentProcessor(Objects.requireNonNull(facilitator),
                                  Objects.requireNonNull(routes)),
             DEFAULT_PRIORITY);
    }

    public X402Adapter(PaymentProcessor processor) {
        this(processor, DEFAULT_PRIORITY);
    }

    public X402Adapter(PaymentProcessor processor, int priority) {
        this.processor = Objects.requireNonNull(processor);
        this.delegateFilter = new PaymentFilter(processor);
        this.priority = priority;
    }

    @Override public String name()     { return NAME; }
    @Override public int    priority() { return priority; }

    @Override
    public boolean detect(HttpServletRequest request) {
        return hasHeader(request, HEADER_X_PAYMENT)
            || hasHeader(request, HEADER_PAYMENT_SIGNATURE);
    }

    @Override
    public CompletableFuture<Map<String, List<String>>> getChallenge(HttpServletRequest request,
                                                                     Object cfg) {
        // cfg is informational only — the underlying PaymentProcessor uses its own
        // route table keyed by URI to find the canonical RouteConfig. Skip when not set.
        if (cfg == null) {
            return CompletableFuture.completedFuture(Map.of());
        }
        return CompletableFuture.supplyAsync(() -> {
            JavaxRequestAdapter req = new JavaxRequestAdapter(request);
            CapturingX402Response captured = new CapturingX402Response();
            try {
                // No X-PAYMENT header on this synthetic request → preHandle takes
                // the "missing payment" branch → respond402 → captured.headers
                // contains PAYMENT-REQUIRED (b64-encoded PaymentRequired JSON).
                processor.preHandle(req, captured);
            } catch (IOException e) {
                // Capture-only — preHandle's IO is purely "writes" to our captured
                // response, which never throws. Defensive log path.
                throw new RuntimeException("x402 challenge capture failed", e);
            }
            Map<String, List<String>> out = new LinkedHashMap<>();
            String payReq = captured.headers.get(HEADER_PAYMENT_REQUIRED);
            if (payReq != null) {
                out.put(HEADER_PAYMENT_REQUIRED, List.of(payReq));
                out.put(HEADER_CORS_EXPOSE, List.of(HEADER_PAYMENT_REQUIRED));
            }
            return out;
        });
    }

    @Override
    public void handle(HttpServletRequest request, HttpServletResponse response,
                       FilterChain chain, Object cfg) throws IOException, ServletException {
        // The embedded PaymentFilter owns verify → chain.doFilter → settle and
        // the X-PAYMENT-RESPONSE header injection. We just hand the request off.
        delegateFilter.doFilter(request, response, chain);
    }

    /** Underlying processor — exposed for advanced config (hooks, executor, …). */
    public PaymentProcessor processor() {
        return processor;
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    private static boolean hasHeader(HttpServletRequest req, String name) {
        String v = req.getHeader(name);
        return v != null && !v.isEmpty();
    }

    /**
     * Minimal {@link X402Response} that records the headers / status / body the
     * {@link PaymentProcessor} writes during {@code preHandle}'s 402 path.
     * Used by {@link #getChallenge} to extract the {@code PAYMENT-REQUIRED}
     * header without coupling a real servlet response.
     */
    static final class CapturingX402Response implements X402Response {
        final Map<String, String> headers = new LinkedHashMap<>();
        final StringBuilder body = new StringBuilder();
        int status = 200;
        String contentType = null;
        boolean committed = false;

        @Override public void setStatus(int s) { this.status = s; }
        @Override public int  getStatus()      { return status; }
        @Override public boolean isCommitted() { return committed; }
        @Override public void setContentType(String ct) { this.contentType = ct; }
        @Override public void setHeader(String name, String value) {
            // Header names from x402 are case-sensitive lookups in our extraction;
            // canonicalize at write time so "PAYMENT-REQUIRED" reads back stably.
            headers.put(name == null ? null : name.toUpperCase(Locale.ROOT) /* keep stable */, value);
            // Defensive: also store the original casing to be safe with downstream lookups.
            headers.put(name, value);
        }
        @Override public void writeBody(String b) { body.append(b); committed = true; }
        @Override public Object unwrap() { return null; }
    }
}
