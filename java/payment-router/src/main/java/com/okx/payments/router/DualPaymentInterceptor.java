package com.okx.payments.router;

import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.payments.router.adapters.MppAdapter;
import com.okx.x402.server.PaymentInterceptor;
import com.okx.x402.server.X402Response;
import com.okx.x402.server.internal.JavaxRequestAdapter;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.web.servlet.HandlerInterceptor;
import org.springframework.web.servlet.ModelAndView;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.IOException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;

/**
 * Spring MVC {@link HandlerInterceptor} that multiplexes <strong>x402 exact</strong>
 * and <strong>MPP session</strong> on the same path pattern (Spring 5 / Spring
 * Boot 2 / Java EE 8).
 *
 * <p>Built for the DEX-style integration: register <em>one</em> interceptor at
 * {@code /api/v6/dex/market/**}, dispatch per-request to the appropriate
 * protocol delegate. Avoids the three failure modes of running the two
 * interceptors back-to-back:
 *
 * <ol>
 *   <li><strong>Cross-protocol rejection</strong> — both delegates run
 *       {@code preHandle} on the same request; whichever doesn't recognize
 *       the headers writes 402/400 and short-circuits the chain, even though
 *       the other had already verified the payment.</li>
 *   <li><strong>Double hook evaluation</strong> — {@code onProtectedRequest}
 *       fires twice per request when the same logic is registered on both
 *       interceptors.</li>
 *   <li><strong>402 race</strong> — first-touch requests with no auth header
 *       race the two delegates' 402 emission paths.</li>
 * </ol>
 *
 * <h3>Dispatch matrix</h3>
 *
 * <table>
 *   <caption>Delegate selected by {@link #pickDelegate} (request carries headers)</caption>
 *   <tr><th>Request shape</th>                                         <th>Delegate</th></tr>
 *   <tr><td>{@code PAYMENT-SIGNATURE} or {@code X-PAYMENT} present</td><td>x402</td></tr>
 *   <tr><td>{@code Authorization: Payment ...} present</td>            <td>MPP</td></tr>
 *   <tr><td>{@code X-Channel-Id} present</td>                          <td>MPP</td></tr>
 *   <tr><td>none, but URI is in MPP route table</td>                   <td>MPP</td></tr>
 *   <tr><td>none, otherwise (default)</td>                             <td>x402</td></tr>
 * </table>
 *
 * <h3>First-touch challenge merging</h3>
 *
 * <p>When the request carries <em>no</em> payment headers and the URL is
 * configured on <strong>both</strong> the MPP route table and the x402
 * processor's route map, {@link #preHandle} short-circuits with a merged
 * 402 envelope containing <em>both</em> a {@code WWW-Authenticate: Payment …}
 * (MPP) header and a {@code PAYMENT-REQUIRED} (x402) header — so the buyer
 * can pay with either protocol. This mirrors the Filter path's
 * {@code ChallengeMerger}. When only one protocol covers the URL, the
 * legacy {@link #pickDelegate} single-pick flow runs unchanged. Opt out
 * via {@link #mergeFirstTouchChallenges(boolean) mergeFirstTouchChallenges(false)}.
 *
 * <h3>Lifecycle</h3>
 *
 * <p>The picked delegate is stored on the request as
 * {@value #ATTR_PICKED_DELEGATE}. {@link #postHandle} and
 * {@link #afterCompletion} read this attribute and route to the same
 * delegate that handled {@code preHandle}, so each protocol's full
 * verify → settle → inject pipeline runs end-to-end without
 * cross-talk.
 *
 * <p>If {@code preHandle} returns {@code false} (delegate wrote 402/403/etc),
 * Spring MVC <strong>does not</strong> call {@code postHandle} or
 * {@code afterCompletion} on this interceptor — same Spring contract as a
 * single interceptor. Nothing special to do here.
 *
 * <h3>Hook coordination</h3>
 *
 * <p>{@code onProtectedRequest} should be configured on each delegate
 * separately (x402's {@code PaymentProcessor.onProtectedRequest} and
 * MPP's {@link MppPaymentInterceptor#onProtectedRequest}) — but the
 * delegates each fire their hook at most once per request, so the DEX-side
 * lambda for {@code paymentExempt} is evaluated exactly once.
 */
public final class DualPaymentInterceptor implements HandlerInterceptor {

    private static final Logger log = LoggerFactory.getLogger(DualPaymentInterceptor.class);

    /** Stash key for the picked delegate, carried preHandle → postHandle → afterCompletion. */
    static final String ATTR_PICKED_DELEGATE = "dual.payment.picked";

    private final PaymentInterceptor    x402Delegate;
    private final MppPaymentInterceptor mppDelegate;
    private boolean mergeFirstTouchChallenges = true;

    public DualPaymentInterceptor(PaymentInterceptor x402Delegate,
                                  MppPaymentInterceptor mppDelegate) {
        this.x402Delegate = Objects.requireNonNull(x402Delegate, "x402Delegate");
        this.mppDelegate  = Objects.requireNonNull(mppDelegate,  "mppDelegate");
    }

    /**
     * Opt out of the first-touch merge behavior (default: on). When disabled, a
     * no-header request to a URL covered by <em>both</em> protocols falls back
     * to the legacy {@link #pickDelegate} matrix (defaults to x402), and the
     * buyer sees only one protocol advertised in the 402 envelope.
     *
     * <p>Use when a deployment intentionally wants single-protocol advertisement
     * even though both protocols are configured.
     */
    public DualPaymentInterceptor mergeFirstTouchChallenges(boolean enabled) {
        this.mergeFirstTouchChallenges = enabled;
        return this;
    }

    public static DualPaymentInterceptor create(PaymentInterceptor x402, MppPaymentInterceptor mpp) {
        return new DualPaymentInterceptor(x402, mpp);
    }

    public PaymentInterceptor    x402Delegate() { return x402Delegate; }
    public MppPaymentInterceptor mppDelegate()  { return mppDelegate; }

    @Override
    public boolean preHandle(HttpServletRequest request,
                             HttpServletResponse response,
                             Object handler) throws Exception {

        // First-touch merge: when the request carries no payment headers AND
        // both protocols cover this URL, emit one 402 with N WWW-Authenticate
        // (MPP, one per option) + PAYMENT-REQUIRED (x402) so the buyer can
        // choose any token under either protocol. Mirrors the Filter path's
        // ChallengeMerger semantics.
        if (mergeFirstTouchChallenges && isFirstTouch(request)) {
            List<String> mppWwwAuth = tryBuildMppChallenges(request);
            String x402Required = tryBuildX402Challenge(request);
            if (!mppWwwAuth.isEmpty() && x402Required != null) {
                writeMergedChallenge(response, mppWwwAuth, x402Required);
                if (log.isDebugEnabled()) {
                    log.debug("[dual] uri={} first-touch → merged 402 (mpp×{} + x402)",
                        request.getRequestURI(), mppWwwAuth.size());
                }
                // Do not set ATTR_PICKED_DELEGATE: post/afterCompletion stay
                // no-op (and Spring won't call them anyway since we returned false).
                return false;
            }
        }

        HandlerInterceptor picked = pickDelegate(request);
        request.setAttribute(ATTR_PICKED_DELEGATE, picked);
        if (log.isDebugEnabled()) {
            log.debug("[dual] uri={} picked={}", request.getRequestURI(),
                picked == x402Delegate ? "x402" : "mpp");
        }
        return picked.preHandle(request, response, handler);
    }

    /** True iff the request carries none of the x402 / MPP payment headers. */
    private static boolean isFirstTouch(HttpServletRequest request) {
        if (header(request, "PAYMENT-SIGNATURE") != null) return false;
        if (header(request, "X-PAYMENT")        != null) return false;
        if (header(request, MppPaymentInterceptor.CHANNEL_ID_HEADER) != null) return false;
        String auth = header(request, "Authorization");
        return auth == null || !auth.toLowerCase(Locale.ROOT).startsWith("payment ");
    }

    /**
     * Build one {@code WWW-Authenticate: Payment …} header value per MPP option
     * configured for this URL. Returns an empty list when MPP does not cover
     * the URL (no route entry) or when challenge construction fails.
     */
    List<String> tryBuildMppChallenges(HttpServletRequest request) {
        MppRouteConfig routes = mppDelegate.routes();
        if (routes == null) return List.of();
        MppRouteConfig.Entry entry = routes.match(request.getRequestURI());
        if (entry == null) return List.of();
        try {
            List<String> out = new ArrayList<>(entry.options().size());
            for (SessionRequest sr : entry.buildSessionRequests()) {
                Challenge ch = mppDelegate.mppServer()
                    .request(entry.realm(), Intent.SESSION, sr);
                out.add(MppAdapter.serializeWwwAuth(ch));
            }
            return out;
        } catch (RuntimeException e) {
            log.warn("[dual] mpp challenge build failed for {}: {}",
                request.getRequestURI(), e.getMessage());
            return List.of();
        }
    }

    /**
     * Build the {@code PAYMENT-REQUIRED} base64 header value for this URL, or
     * return {@code null} if x402 does not cover it (no route entry on the
     * underlying processor) or a hook short-circuited the request.
     *
     * <p>Drives the x402 SDK's own {@code preHandle} against a non-committing
     * capturing response so we can harvest the header without writing it to
     * the real {@link HttpServletResponse}. Same trick {@code X402Adapter} uses
     * on the Filter path.
     */
    String tryBuildX402Challenge(HttpServletRequest request) {
        if (x402Delegate.processor() == null) return null;
        JavaxRequestAdapter xReq = new JavaxRequestAdapter(request);
        CapturingX402Response captured = new CapturingX402Response();
        try {
            x402Delegate.processor().preHandle(xReq, captured);
        } catch (IOException | RuntimeException e) {
            // CapturingX402Response.writeBody throws nothing; defensive only.
            log.warn("[dual] x402 challenge capture failed for {}: {}",
                request.getRequestURI(), e.getMessage());
            return null;
        }
        return captured.headers.get("PAYMENT-REQUIRED");
    }

    /** Write the merged 402 envelope and commit the response. */
    private void writeMergedChallenge(HttpServletResponse response,
                                      List<String> mppWwwAuthLines,
                                      String x402Required) throws IOException {
        if (response.isCommitted()) return;
        response.reset();
        response.setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        for (String line : mppWwwAuthLines) {
            response.addHeader("WWW-Authenticate", line);
        }
        response.setHeader("PAYMENT-REQUIRED", x402Required);
        response.setHeader("Access-Control-Expose-Headers", "PAYMENT-REQUIRED");
        response.setContentType("application/problem+json");
        response.getWriter().write(
            "{\"type\":\"https://paymentauth.org/problems/payment-required\","
            + "\"status\":402,\"title\":\"Payment Required\"}");
    }

    @Override
    public void postHandle(HttpServletRequest request,
                           HttpServletResponse response,
                           Object handler,
                           ModelAndView modelAndView) throws Exception {
        HandlerInterceptor picked = (HandlerInterceptor) request.getAttribute(ATTR_PICKED_DELEGATE);
        if (picked != null) {
            picked.postHandle(request, response, handler, modelAndView);
        }
    }

    @Override
    public void afterCompletion(HttpServletRequest request,
                                HttpServletResponse response,
                                Object handler,
                                Exception ex) throws Exception {
        HandlerInterceptor picked = (HandlerInterceptor) request.getAttribute(ATTR_PICKED_DELEGATE);
        if (picked != null) {
            picked.afterCompletion(request, response, handler, ex);
        }
    }

    /** Pick exactly one delegate per request — see class Javadoc dispatch matrix. */
    HandlerInterceptor pickDelegate(HttpServletRequest request) {
        // 1. Explicit x402 v2 / v1 header → x402
        if (header(request, "PAYMENT-SIGNATURE") != null) return x402Delegate;
        if (header(request, "X-PAYMENT")        != null) return x402Delegate;

        // 2. MPP credential headers
        String auth = header(request, "Authorization");
        if (auth != null && auth.toLowerCase(Locale.ROOT).startsWith("payment ")) {
            return mppDelegate;
        }
        if (header(request, MppPaymentInterceptor.CHANNEL_ID_HEADER) != null) {
            return mppDelegate;
        }

        // 3. No auth header — first-touch. Prefer MPP only if the URI is in the
        //    MPP route table (e.g. /session/manage); otherwise default to x402.
        if (mppDelegate.matches(request)) {
            return mppDelegate;
        }
        return x402Delegate;
    }

    private static String header(HttpServletRequest req, String name) {
        String v = req.getHeader(name);
        return (v == null || v.isBlank()) ? null : v;
    }

    /**
     * In-memory {@link X402Response} that records what the x402
     * {@code PaymentProcessor.preHandle} would have written for a missing-payment
     * request, so we can extract the {@code PAYMENT-REQUIRED} header without
     * committing the real servlet response. Mirrors the wrapper in
     * {@code X402Adapter}.
     */
    static final class CapturingX402Response implements X402Response {
        final Map<String, String> headers = new LinkedHashMap<>();
        final StringBuilder body = new StringBuilder();
        int status = 200;
        String contentType;
        boolean committed;

        @Override public void setStatus(int s)              { this.status = s; }
        @Override public int  getStatus()                   { return status; }
        @Override public boolean isCommitted()              { return committed; }
        @Override public void setContentType(String ct)     { this.contentType = ct; }
        @Override public void setHeader(String name, String value) {
            headers.put(name, value);
        }
        @Override public void writeBody(String b) { body.append(b); committed = true; }
        @Override public Object unwrap() { return null; }
    }
}
