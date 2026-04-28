// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.internal.BufferedHttpServletResponse;
import com.okx.x402.server.internal.JakartaRequestAdapter;
import com.okx.x402.server.internal.JakartaResponseAdapter;

import jakarta.servlet.Filter;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletException;
import jakarta.servlet.ServletRequest;
import jakarta.servlet.ServletResponse;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.Map;

/**
 * Servlet filter adapter for {@link PaymentProcessor} (Jakarta EE 9+ /
 * Spring Boot 3). Plug into any servlet container or Spring Boot via
 * {@code FilterRegistrationBean}; order relative to other filters is
 * controlled by the container's registration order.
 *
 * <p>For Spring MVC projects that already use {@code HandlerInterceptor}s
 * for billing / auth, {@link PaymentInterceptor} is an alternative so all
 * interceptors share the same execution layer and order is controlled
 * uniformly via {@code InterceptorRegistry.order()}.
 */
public class PaymentFilter implements Filter {

    private final PaymentProcessor processor;

    public PaymentFilter(PaymentProcessor processor) {
        this.processor = processor;
    }

    public static PaymentFilter create(PaymentProcessor processor) {
        return new PaymentFilter(processor);
    }

    public static PaymentFilter create(FacilitatorClient facilitator,
                                       Map<String, PaymentProcessor.RouteConfig> routes) {
        return new PaymentFilter(new PaymentProcessor(facilitator, routes));
    }

    /** Expose the underlying processor for additional configuration. */
    public PaymentProcessor processor() {
        return processor;
    }

    @Override
    public void doFilter(ServletRequest req, ServletResponse res,
                         FilterChain chain)
            throws IOException, ServletException {

        if (!(req instanceof HttpServletRequest request)
                || !(res instanceof HttpServletResponse response)) {
            chain.doFilter(req, res);
            return;
        }

        JakartaRequestAdapter xReq = new JakartaRequestAdapter(request);

        // Fast-path: non-paid routes skip all buffering and settle logic.
        // This keeps the filter cheap when url-patterns overshoot the set of
        // routes actually registered in the PaymentProcessor.
        if (processor.matchRoute(xReq) == null) {
            chain.doFilter(req, res);
            return;
        }

        // Paid route: buffer the downstream handler's body so the settlement
        // step (which sets PAYMENT-RESPONSE after the handler returns) still
        // operates on an uncommitted response. Without this, the servlet
        // container often commits headers as soon as the handler writes, and
        // the PAYMENT-RESPONSE header set in postHandle is silently dropped.
        BufferedHttpServletResponse wrapped = new BufferedHttpServletResponse(response);
        JakartaResponseAdapter xRes = new JakartaResponseAdapter(wrapped);

        PaymentProcessor.VerifyResult result = processor.preHandle(xReq, xRes);

        if (result == null) {
            // 402/403/500 already written to the buffered view — flush it.
            wrapped.commit();
            return;
        }
        if (!result.isVerified()) {
            // Route matched but preHandle granted access (e.g. onProtectedRequest
            // hook returned GRANT_ACCESS). Still run through the buffered
            // wrapper for consistency; no settle step needed.
            chain.doFilter(req, wrapped);
            wrapped.commit();
            return;
        }

        // Verified — run the business handler against the buffered response,
        // then settle, then flush. If the handler throws, skip commit() so
        // the container's error page handling can write to the still-uncommitted
        // underlying response.
        chain.doFilter(req, wrapped);
        try {
            processor.postHandle(result, xReq, xRes);
        } finally {
            wrapped.commit();
        }
    }
}
