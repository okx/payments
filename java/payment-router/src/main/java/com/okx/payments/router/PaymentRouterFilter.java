package com.okx.payments.router;

import javax.servlet.Filter;
import javax.servlet.FilterChain;
import javax.servlet.ServletException;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.util.List;
import java.util.Map;

/**
 * Servlet {@link Filter} entry point for the payment router.
 *
 * <p>Per request:
 * <ol>
 *   <li>match the request URL against {@link PaymentRouterConfig#routes()} (first-match-wins)</li>
 *   <li>if unmatched, pass through to the chain</li>
 *   <li>else, run {@link ProtocolDetector} — if any protocol matches, hand off to its {@code handle}</li>
 *   <li>else, merge concurrent challenges and emit 402 with multi-line headers</li>
 * </ol>
 */
public final class PaymentRouterFilter implements Filter {

    private static final Logger log = LoggerFactory.getLogger(PaymentRouterFilter.class);

    private final PaymentRouterConfig config;
    private final RouteMatcher routeMatcher;
    private final ProtocolDetector detector;
    private final ChallengeMerger merger;

    public PaymentRouterFilter(PaymentRouterConfig config) {
        this.config = config;
        this.routeMatcher = new RouteMatcher(config.routes());
        this.detector = new ProtocolDetector(config.protocols());
        this.merger = new ChallengeMerger(config.protocols(), config.onError());
    }

    @Override
    public void doFilter(ServletRequest req, ServletResponse resp, FilterChain chain)
        throws IOException, ServletException {
        if (!(req instanceof HttpServletRequest httpReq) || !(resp instanceof HttpServletResponse httpResp)) {
            chain.doFilter(req, resp);
            return;
        }

        RouteMatcher.Match m = routeMatcher.match(httpReq.getMethod(), httpReq.getRequestURI());
        if (m == null) {
            chain.doFilter(req, resp);
            return;
        }

        ProtocolAdapter adapter = detector.detect(httpReq);
        if (adapter != null) {
            try {
                Object cfg = m.config().get(adapter.name());
                adapter.handle(httpReq, httpResp, chain, cfg);
            } catch (RuntimeException | IOException | ServletException e) {
                if (config.onError() != null) config.onError().accept(e, adapter.name());
                if (!httpResp.isCommitted()) {
                    httpResp.sendError(HttpServletResponse.SC_INTERNAL_SERVER_ERROR, "handler failed");
                }
            }
            return;
        }

        // No payment header — emit merged 402.
        Map<String, List<String>> challenges = merger.merge(httpReq, m.config());
        httpResp.setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        for (Map.Entry<String, List<String>> e : challenges.entrySet()) {
            for (String v : e.getValue()) {
                httpResp.addHeader(e.getKey(), v);   // multi-line same-name header per spec
            }
        }
        httpResp.setContentType("application/problem+json");
        httpResp.getWriter().write("{\"type\":\"https://paymentauth.org/problems/payment-required\","
            + "\"status\":402,\"title\":\"Payment Required\"}");
    }
}
