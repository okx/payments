// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server.internal;

import com.okx.x402.server.X402Request;

import jakarta.servlet.http.HttpServletRequest;

/** Adapts a Jakarta {@link HttpServletRequest} to the servlet-agnostic core. */
public final class JakartaRequestAdapter implements X402Request {

    private final HttpServletRequest delegate;

    public JakartaRequestAdapter(HttpServletRequest delegate) {
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

    @Override
    public HttpServletRequest unwrap() {
        return delegate;
    }
}
