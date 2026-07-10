// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server;

import java.io.IOException;

/**
 * Servlet-agnostic view of an HTTP response used by {@link PaymentProcessor}.
 *
 * <p>Adapter modules (okxweb3-app-x402-jakarta, okxweb3-app-x402-javax) wrap the native
 * servlet response types so the core logic stays independent of the
 * javax/jakarta namespace.
 */
public interface X402Response {

    int SC_PAYMENT_REQUIRED = 402;
    int SC_INTERNAL_SERVER_ERROR = 500;

    void setStatus(int status);

    int getStatus();

    boolean isCommitted();

    void setContentType(String contentType);

    void setHeader(String name, String value);

    /** Write the given string to the response body. */
    void writeBody(String body) throws IOException;

    /** The native response object; see {@link X402Request#unwrap()}. */
    Object unwrap();
}
