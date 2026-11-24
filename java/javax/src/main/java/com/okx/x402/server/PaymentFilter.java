// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.internal.BufferedHttpServletResponse;
import com.okx.x402.server.internal.JavaxRequestAdapter;
import com.okx.x402.server.internal.JavaxResponseAdapter;

import javax.servlet.Filter;
import javax.servlet.FilterChain;
import javax.servlet.ServletException;
import javax.servlet.ServletRequest;
import javax.servlet.ServletResponse;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;

import java.io.IOException;
import java.util.Map;

/**
 * Servlet filter adapter for {@link PaymentProcessor} (Java EE 8 /
 * Spring Boot 2). Plug into any servlet container or Spring Boot via
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

        if (!(req instanceof HttpServletRequest) || !(res instanceof HttpServletResponse)) {
            chain.doFilter(req, res);
            return;
        }

        HttpServletRequest request = (HttpServletRequest) req;
        HttpServletResponse response = (HttpServletResponse) res;

        JavaxRequestAdapter xReq = new JavaxRequestAdapter(request);

        // Fast-path: non-paid routes skip all buffering and settle logic.
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
        JavaxResponseAdapter xRes = new JavaxResponseAdapter(wrapped);

        PaymentProcessor.VerifyResult result = processor.preHandle(xReq, xRes);

        if (result == null) {
            // 402/403/500 already written to the buffered view — flush it.
            wrapped.commit();
            return;
        }
        if (!result.isVerified()) {
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
