// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

/**
 * Servlet-agnostic view of an HTTP request used by {@link PaymentProcessor}.
 *
 * <p>Adapter modules (okxweb3-app-x402-jakarta, okxweb3-app-x402-javax) wrap the native
 * servlet request types so the core logic stays independent of the
 * javax/jakarta namespace.
 */
public interface X402Request {

    String getMethod();

    /** Path portion only (no scheme/host/query), e.g. {@code /weather}. */
    String getRequestURI();

    /** Absolute URL including scheme and host, e.g. {@code https://api.example/weather}. */
    String getRequestURL();

    String getHeader(String name);

    Object getAttribute(String name);

    /**
     * The request body as a UTF-8 string. Used by subscription lifecycle operation routes
     * (cancel / cancel-pending-change) that relay a buyer-signed auth carried in the body.
     * Adapters should read the underlying stream once and cache the result. Default throws —
     * adapters that predate this method simply cannot serve operation routes.
     */
    default String getBody() throws java.io.IOException {
        throw new UnsupportedOperationException(
                "getBody() not supported by this X402Request adapter");
    }

    /**
     * The native request object (HttpServletRequest in jakarta or javax
     * namespace, depending on which adapter module is in use). Exposed so
     * user callbacks (e.g. {@code DynamicPrice},
     * {@link PaymentHooks.OnProtectedRequestHook}) can cast to the native
     * type when they need access to request details beyond what this
     * interface exposes.
     */
    Object unwrap();
}
