package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import org.springframework.web.servlet.HandlerInterceptor;
import org.springframework.web.servlet.ModelAndView;

import java.util.Map;

/**
 * Spring MVC interceptor adapter for {@link PaymentProcessor}.
 *
 * <p>Use this when your project uses Spring MVC interceptors for billing / auth,
 * so all interceptors share the same execution layer and order is controlled
 * via {@code InterceptorRegistry.order()}.
 *
 * <p>Lifecycle:
 * <ul>
 *   <li>{@code preHandle} — route match, skip check, verify payment</li>
 *   <li>(handler runs)</li>
 *   <li>{@code postHandle} — settle payment (sync or async)</li>
 * </ul>
 */
public class PaymentInterceptor implements HandlerInterceptor {

    /** @see PaymentProcessor#PAYMENT_EXEMPT_ATTRIBUTE */
    public static final String PAYMENT_EXEMPT_ATTRIBUTE =
            PaymentProcessor.PAYMENT_EXEMPT_ATTRIBUTE;

    private static final String ATTR_VERIFY_RESULT =
            "payment.internal.verifyResult";

    private final PaymentProcessor processor;

    public PaymentInterceptor(PaymentProcessor processor) {
        this.processor = processor;
    }

    public static PaymentInterceptor create(PaymentProcessor processor) {
        return new PaymentInterceptor(processor);
    }

    public static PaymentInterceptor create(
            FacilitatorClient facilitator,
            Map<String, PaymentProcessor.RouteConfig> routes) {
        return new PaymentInterceptor(
                new PaymentProcessor(facilitator, routes));
    }

    /** Expose the underlying processor for additional configuration. */
    public PaymentProcessor processor() {
        return processor;
    }

    @Override
    public boolean preHandle(HttpServletRequest request,
                             HttpServletResponse response,
                             Object handler) throws Exception {

        PaymentProcessor.VerifyResult result =
                processor.preHandle(request, response);

        if (result == null) {
            // 402/500 already written
            return false;
        }
        if (!result.isVerified()) {
            // Not a paid route or skipped — continue to handler
            return true;
        }

        // Store for postHandle
        request.setAttribute(ATTR_VERIFY_RESULT, result);
        return true;
    }

    @Override
    public void postHandle(HttpServletRequest request,
                           HttpServletResponse response,
                           Object handler,
                           ModelAndView modelAndView) throws Exception {

        PaymentProcessor.VerifyResult result =
                (PaymentProcessor.VerifyResult)
                        request.getAttribute(ATTR_VERIFY_RESULT);

        processor.postHandle(result, request, response);
    }
}
