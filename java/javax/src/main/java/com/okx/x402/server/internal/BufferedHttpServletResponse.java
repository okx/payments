// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server.internal;

import javax.servlet.ServletOutputStream;
import javax.servlet.WriteListener;
import javax.servlet.http.HttpServletResponse;
import javax.servlet.http.HttpServletResponseWrapper;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintWriter;
import java.nio.charset.Charset;
import java.nio.charset.StandardCharsets;

/**
 * Response wrapper that captures the downstream handler's body, status,
 * flushBuffer() calls, and isCommitted() checks into an in-memory buffer so
 * that {@link com.okx.x402.server.PaymentFilter} can run settlement
 * <em>after</em> the handler returns but <em>before</em> anything reaches
 * the socket. Without this, the servlet container commits headers as soon
 * as the handler writes past the response buffer (or calls flushBuffer()),
 * and the {@code PAYMENT-RESPONSE} header written by {@code postHandle} is
 * silently dropped on a committed response.
 *
 * <p>Mirrors the raw-response buffering pattern used by the TypeScript SDK's
 * Express / Fastify adapters (buffer writeHead/write/end, run settlement,
 * then replay).
 *
 * <p>Call {@link #commit()} to replay the captured state onto the underlying
 * response. Once committed, further writes pass through unchanged.
 *
 * <h2>Known limitations</h2>
 * <ul>
 *   <li><b>{@code sendError(...)} is not buffered.</b> The
 *       {@link javax.servlet.http.HttpServletResponseWrapper} default
 *       implementation forwards {@link HttpServletResponse#sendError(int)}
 *       and {@link HttpServletResponse#sendError(int, String)} straight to
 *       the underlying response, which immediately commits it. As a result,
 *       any handler that signals failure via {@code sendError(...)} on a
 *       paid route will commit the response before
 *       {@link com.okx.x402.server.PaymentProcessor#postHandle
 *       PaymentProcessor.postHandle} runs and the
 *       {@code PAYMENT-RESPONSE} header will be silently dropped. The flow
 *       is still safe — the upstream response status is &gt;= 400 so
 *       {@code postHandle} short-circuits and no settlement happens — but
 *       callers that want a settlement proof on the response must surface
 *       business errors via {@code setStatus(...) + writeBody(...)} instead
 *       of {@code sendError(...)}.</li>
 *   <li><b>Async I/O is not supported.</b> The buffered
 *       {@code ServletOutputStream} is blocking; any handler that opts into
 *       Servlet 3.x non-blocking I/O via {@code setWriteListener(...)} will
 *       not receive the listener callbacks. The buffered wrapper is
 *       designed for the synchronous Filter -&gt; handler -&gt; postHandle
 *       flow only.</li>
 *   <li><b>{@code setContentLength(...)} is deferred.</b> Any
 *       Content-Length set by the handler is dropped; the real value is
 *       implicit in the byte count replayed at {@link #commit()}.</li>
 * </ul>
 */
public final class BufferedHttpServletResponse extends HttpServletResponseWrapper {

    private final ByteArrayOutputStream buffer = new ByteArrayOutputStream();
    private ServletOutputStream outputStream;
    private PrintWriter writer;
    private boolean committed;

    public BufferedHttpServletResponse(HttpServletResponse response) {
        super(response);
    }

    @Override
    public ServletOutputStream getOutputStream() throws IOException {
        if (committed) {
            return super.getOutputStream();
        }
        if (writer != null) {
            throw new IllegalStateException(
                    "getWriter() already called on this response");
        }
        if (outputStream == null) {
            outputStream = new BufferingServletOutputStream(buffer);
        }
        return outputStream;
    }

    @Override
    public PrintWriter getWriter() throws IOException {
        if (committed) {
            return super.getWriter();
        }
        if (outputStream != null) {
            throw new IllegalStateException(
                    "getOutputStream() already called on this response");
        }
        if (writer == null) {
            Charset cs = resolveCharset();
            writer = new PrintWriter(new OutputStreamWriter(buffer, cs), false);
        }
        return writer;
    }

    private Charset resolveCharset() {
        String enc = getCharacterEncoding();
        if (enc == null || enc.isEmpty()) {
            return StandardCharsets.UTF_8;
        }
        try {
            return Charset.forName(enc);
        } catch (RuntimeException e) {
            return StandardCharsets.UTF_8;
        }
    }

    @Override
    public void flushBuffer() throws IOException {
        if (writer != null) {
            writer.flush();
        }
    }

    @Override
    public boolean isCommitted() {
        return committed || super.isCommitted();
    }

    @Override
    public void reset() {
        if (committed) {
            super.reset();
            return;
        }
        super.reset();
        resetBuffer();
    }

    @Override
    public void resetBuffer() {
        if (committed) {
            super.resetBuffer();
            return;
        }
        buffer.reset();
        outputStream = null;
        writer = null;
    }

    @Override
    public void setContentLength(int len) {
        if (committed) {
            super.setContentLength(len);
        }
    }

    @Override
    public void setContentLengthLong(long len) {
        if (committed) {
            super.setContentLengthLong(len);
        }
    }

    /**
     * Replay buffered status/headers/body onto the underlying response.
     * Safe to call more than once — subsequent calls are no-ops.
     */
    public void commit() throws IOException {
        if (committed) {
            return;
        }
        if (writer != null) {
            writer.flush();
        }
        byte[] body = buffer.toByteArray();
        boolean usedWriter = writer != null;
        committed = true;

        if (body.length > 0) {
            HttpServletResponse real = (HttpServletResponse) getResponse();
            if (usedWriter) {
                java.io.PrintWriter realWriter = real.getWriter();
                if (realWriter != null) {
                    realWriter.write(new String(body, resolveCharset()));
                    realWriter.flush();
                } else {
                    return;
                }
            } else {
                javax.servlet.ServletOutputStream out = real.getOutputStream();
                if (out != null) {
                    out.write(body);
                    out.flush();
                }
            }
        }
        try {
            getResponse().flushBuffer();
        } catch (RuntimeException ignored) {
            // Lenient mock safety.
        }
    }

    public int capturedBodyLength() {
        if (writer != null) {
            writer.flush();
        }
        return buffer.size();
    }

    private static final class BufferingServletOutputStream extends ServletOutputStream {
        private final ByteArrayOutputStream sink;

        BufferingServletOutputStream(ByteArrayOutputStream sink) {
            this.sink = sink;
        }

        @Override
        public void write(int b) {
            sink.write(b);
        }

        @Override
        public void write(byte[] b, int off, int len) {
            sink.write(b, off, len);
        }

        @Override
        public boolean isReady() {
            return true;
        }

        @Override
        public void setWriteListener(WriteListener listener) {
        }
    }
}
