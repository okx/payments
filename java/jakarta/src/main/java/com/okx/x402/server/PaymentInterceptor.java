// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.server.internal.JakartaRequestAdapter;
import com.okx.x402.server.internal.JakartaResponseAdapter;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import org.springframework.web.servlet.HandlerInterceptor;
import org.springframework.web.servlet.ModelAndView;

import java.util.Map;

/**
 * Spring MVC interceptor adapter for {@link PaymentProcessor} (Spring 6 /
 * Spring Boot 3 / Jakarta EE 9+).
 *
 * <p>Use this when your project uses Spring MVC interceptors for billing /
 * auth, so all interceptors share the same execution layer and order is
 * controlled uniformly via {@code InterceptorRegistry.order()}.
 *
 * <p>Lifecycle:
 * <ul>
 *   <li>{@code preHandle} — route match, verify payment; returns
 *       {@code false} (and writes 402/403/500) on reject</li>
 *   <li>(handler runs)</li>
 *   <li>{@code postHandle} — settle payment (sync or async)</li>
 * </ul>
 *
 * <h2>Choosing between {@link PaymentFilter} and PaymentInterceptor</h2>
 *
 * <p>For most Spring REST APIs prefer {@link PaymentFilter} over this
 * interceptor. {@link PaymentFilter} wraps the response in
 * {@link com.okx.x402.server.internal.BufferedHttpServletResponse} so the
 * settlement step in {@code postHandle} can still attach the
 * {@code PAYMENT-RESPONSE} header after the handler has returned. This
 * interceptor talks to the raw {@code HttpServletResponse} and is therefore
 * subject to a Spring-MVC quirk:
 *
 * <ul>
 *   <li>For {@code @ResponseBody} / {@code @RestController} handlers the
 *       message converter writes the response body to the socket
 *       <em>during</em> {@code RequestMappingHandlerAdapter.handle(...)},
 *       which means the response is already committed by the time
 *       {@link org.springframework.web.servlet.HandlerInterceptor#postHandle
 *       HandlerInterceptor.postHandle} runs. {@code setHeader(...)} on a
 *       committed response is a silent no-op per the servlet spec, so the
 *       {@code PAYMENT-RESPONSE} header set by
 *       {@link PaymentProcessor#postHandle PaymentProcessor.postHandle}
 *       will be silently dropped. The settlement itself still happens, but
 *       the buyer client will not see the settlement-proof header.</li>
 *   <li>{@code @Controller} flows that return a view name (the body is
 *       written later, during view rendering) and async / streaming flows
 *       that have not yet committed are unaffected.</li>
 * </ul>
 *
 * <p>If you need a settlement proof on the response and you are using
 * {@code @RestController} / {@code @ResponseBody}, switch to
 * {@link PaymentFilter} (it can be registered alongside Spring's interceptor
 * stack via {@code FilterRegistrationBean} without losing ordering control).
 */
public class PaymentInterceptor implements HandlerInterceptor {

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

        JakartaRequestAdapter xReq = new JakartaRequestAdapter(request);
        JakartaResponseAdapter xRes = new JakartaResponseAdapter(response);

        PaymentProcessor.VerifyResult result = processor.preHandle(xReq, xRes);

        if (result == null) {
            // 402/403/500 already written
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

        if (result == null) {
            return;
        }

        JakartaRequestAdapter xReq = new JakartaRequestAdapter(request);
        JakartaResponseAdapter xRes = new JakartaResponseAdapter(response);
        processor.postHandle(result, xReq, xRes);
    }
}
