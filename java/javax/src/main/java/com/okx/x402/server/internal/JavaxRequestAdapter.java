// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server.internal;

import com.okx.x402.server.X402Request;

import javax.servlet.http.HttpServletRequest;

/** Adapts a javax {@link HttpServletRequest} to the servlet-agnostic core. */
public final class JavaxRequestAdapter implements X402Request {

    private final HttpServletRequest delegate;

    public JavaxRequestAdapter(HttpServletRequest delegate) {
        this.delegate = delegate;
    }

    @Override
    public String getMethod() {
        return delegate.getMethod();
    }

    @Override
    public String getRequestURI() {
        return delegate.getRequestURI();
    }

    @Override
    public String getRequestURL() {
        StringBuffer url = delegate.getRequestURL();
        return url == null ? null : url.toString();
    }

    @Override
    public String getHeader(String name) {
        return delegate.getHeader(name);
    }

    @Override
    public Object getAttribute(String name) {
        return delegate.getAttribute(name);
    }

    private String cachedBody;

    @Override
    public String getBody() throws java.io.IOException {
        if (cachedBody == null) {
            try (java.io.InputStream in = delegate.getInputStream()) {
                cachedBody = new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8);
            }
        }
        return cachedBody;
    }

    @Override
    public HttpServletRequest unwrap() {
        return delegate;
    }
}
