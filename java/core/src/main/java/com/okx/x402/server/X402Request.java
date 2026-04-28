// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

/**
 * Servlet-agnostic view of an HTTP request used by {@link PaymentProcessor}.
 *
 * <p>Adapter modules (x402-java-jakarta, x402-java-javax) wrap the native
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
     * The native request object (HttpServletRequest in jakarta or javax
     * namespace, depending on which adapter module is in use). Exposed so
     * user callbacks (e.g. {@code DynamicPrice},
     * {@link PaymentHooks.OnProtectedRequestHook}) can cast to the native
     * type when they need access to request details beyond what this
     * interface exposes.
     */
    Object unwrap();
}
