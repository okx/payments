package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;

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
 * Servlet filter adapter for {@link PaymentProcessor}.
 *
 * <p>For Spring MVC projects that use interceptors, prefer
 * {@link PaymentInterceptor} so billing and payment share the same
 * execution layer and ordering via {@code InterceptorRegistry.order()}.
 */
public class PaymentFilter implements Filter {

    /** @see PaymentProcessor#PAYMENT_EXEMPT_ATTRIBUTE */
    public static final String PAYMENT_EXEMPT_ATTRIBUTE =
            PaymentProcessor.PAYMENT_EXEMPT_ATTRIBUTE;

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

        PaymentProcessor.VerifyResult result =
                processor.preHandle(request, response);

        if (result == null) {
            // 402/500 already written
            return;
        }
        if (!result.isVerified()) {
            // Not a paid route or skipped — pass through
            chain.doFilter(req, res);
            return;
        }

        // Execute business handler
        chain.doFilter(req, res);

        // Settle
        processor.postHandle(result, request, response);
    }
}
