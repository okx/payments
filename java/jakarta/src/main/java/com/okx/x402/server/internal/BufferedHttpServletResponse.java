// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.server.internal;

import jakarta.servlet.ServletOutputStream;
import jakarta.servlet.WriteListener;
import jakarta.servlet.http.HttpServletResponse;
import jakarta.servlet.http.HttpServletResponseWrapper;

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
 *       {@link jakarta.servlet.http.HttpServletResponseWrapper} default
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
        // Intentionally do NOT flush through to the underlying response —
        // that is the whole point of this wrapper. Keep the writer's
        // internal buffer flushed into `buffer` so captured bytes stay
        // fresh, but do not commit the real response.
        if (writer != null) {
            writer.flush();
        }
    }

    @Override
    public boolean isCommitted() {
        // Report uncommitted while we hold the body in-memory. Letting the
        // handler believe the response is still open is exactly what allows
        // postHandle to later attach PAYMENT-RESPONSE.
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
        // Defer — the real Content-Length will be set (or omitted) when we
        // replay the buffered body. Blocking the downstream setter here
        // prevents a stale length leaking to the client if, say, postHandle
        // appends additional bytes (currently it doesn't, but keep the
        // invariant tight).
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
     *
     * <p>Respects the servlet rule that a response is either written via
     * {@code getWriter()} OR {@code getOutputStream()} but never both: if
     * the handler used a writer we replay through the real response's
     * writer, otherwise the real stream. Falls back to the stream if the
     * underlying response is a mock that stubs neither (lenient test
     * doubles would otherwise NPE on a null result).
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
                    // Nothing to write to — treat as no-op (matches the
                    // expectation of lenient unit-test mocks).
                    return;
                }
            } else {
                jakarta.servlet.ServletOutputStream out = real.getOutputStream();
                if (out != null) {
                    out.write(body);
                    out.flush();
                }
            }
        }
        try {
            getResponse().flushBuffer();
        } catch (RuntimeException ignored) {
            // Mock responses frequently leave flushBuffer unstubbed; don't
            // fail the real flow because of a defensive call.
        }
    }

    /**
     * Current captured body length. Useful for tests / diagnostics.
     */
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
            // Async I/O not supported on the buffered view — callers only
            // interact with it during the synchronous handler flow.
        }
    }
}
