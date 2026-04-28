// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server.internal;

import com.okx.x402.server.X402Response;

import jakarta.servlet.http.HttpServletResponse;

import java.io.IOException;

/** Adapts a Jakarta {@link HttpServletResponse} to the servlet-agnostic core. */
public final class JakartaResponseAdapter implements X402Response {

    private final HttpServletResponse delegate;

    public JakartaResponseAdapter(HttpServletResponse delegate) {
        this.delegate = delegate;
    }

    @Override
    public void setStatus(int status) {
        delegate.setStatus(status);
    }

    @Override
    public int getStatus() {
        return delegate.getStatus();
    }

    @Override
    public boolean isCommitted() {
        return delegate.isCommitted();
    }

    @Override
    public void setContentType(String contentType) {
        delegate.setContentType(contentType);
    }

    @Override
    public void setHeader(String name, String value) {
        delegate.setHeader(name, value);
    }

    @Override
    public void writeBody(String body) throws IOException {
        // Force UTF-8 before obtaining the writer so '₮' (U+20AE) and other
        // non-ISO-8859-1 chars survive the wire format. Must precede getWriter().
        delegate.setCharacterEncoding("UTF-8");
        delegate.getWriter().write(body);
    }

    @Override
    public HttpServletResponse unwrap() {
        return delegate;
    }
}
