package com.okx.payments.router;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.errors.InsufficientBalanceError;
import com.okx.payments.mpp.errors.MppError;
import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.seller.SessionStore;
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.payments.mpp.server.SessionResult;
import com.okx.payments.router.adapters.MppAdapter;
import com.okx.x402.server.PaymentHooks.ProtectedRequestResult;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.web.servlet.HandlerInterceptor;
import org.springframework.web.servlet.ModelAndView;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.IOException;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.function.BiFunction;
import java.util.function.Function;

/**
 * Spring MVC {@link HandlerInterceptor} adapter for MPP session — Spring 5 /
 * Spring Boot 2 / Java EE 8 (javax.servlet).
 *
 * <p>Mirrors {@code com.okx.x402.server.PaymentInterceptor}'s shape so DEX-style
 * teams that already wire x402 via interceptor can plug MPP in the same way.
 *
 * <h3>Lifecycle</h3>
 *
 * <pre>
 * preHandle:
 *   route = MppRouteConfig.match(uri)
 *   if route == null              → return true (path not MPP-managed)
 *   if route.kind == SESSION_MANAGE
 *       decode Authorization: Payment &lt;b64&gt;
 *       evmSessionMethod.verifySession(credential, request)  → write 200/4xx
 *       return false                                        (terminal)
 *   if route.kind == RESOURCE
 *       hookResult = onProtectedRequest(req, entry)         (paymentExempt etc)
 *       if grantAccess  → return true   (no deduct)
 *       if abort        → write 403,    return false
 *       channelId = channelIdResolver(req)
 *       if blank        → write 400,    return false
 *       stash {channelId, entry} on request
 *       return true                                       (continue to handler)
 *
 * postHandle:
 *   ctx = req.attribute("mpp.context")
 *   if ctx == null || resp.status &gt;= 400 → noop
 *   try {
 *     deduct = mppServer.deductFromChannel(ctx.channelId, ctx.entry.price)
 *     resp.setHeader X-Spent / X-Units / Payment-Receipt
 *   } catch (InsufficientBalanceError) {
 *     // 70015 — channel exhausted
 *     resp.reset(); resp.setStatus(402);
 *     resp.setHeader WWW-Authenticate: Payment ...
 *     write problem+json
 *   }
 * </pre>
 *
 * <h3>postHandle vs afterCompletion</h3>
 *
 * <p>For {@code @RestController} returning a body, Spring's
 * {@code RequestResponseBodyMethodProcessor} writes via the message converter
 * <em>after</em> the handler returns but <em>before</em> {@code postHandle}
 * fires (when the dispatcher's view-rendering phase begins). For controller
 * methods returning a view name, {@code postHandle} runs <em>before</em> view
 * rendering. In practice both flows let us
 * {@code resp.reset()} cleanly when {@code !resp.isCommitted()} — same as
 * x402's {@code PaymentInterceptor.postHandle} which calls
 * {@link #processor processor.postHandle(...)} at the same point.
 *
 * <h3>Hooks</h3>
 *
 * <p>Reuses x402's {@link ProtectedRequestResult} types so DEX can register
 * one lambda for both interceptors (read {@code paymentExempt} request
 * attribute, etc.):
 *
 * <pre>{@code
 * mppPaymentInterceptor.onProtectedRequest((req, entry) -> {
 *     if (Boolean.TRUE.equals(req.getAttribute("paymentExempt")))
 *         return ProtectedRequestResult.grantAccess();
 *     return ProtectedRequestResult.proceed();
 * });
 * }</pre>
 *
 * <h3>What this interceptor does NOT do</h3>
 * <ul>
 *   <li>Does not handle x402 protocol — pair it with x402's
 *       {@code PaymentInterceptor} (or use {@link DualPaymentInterceptor}
 *       to multiplex both schemes on the same path pattern).</li>
 *   <li>Does not own the persistent {@code SessionStore} — pluggable via
 *       {@code MppServer.builder().sessionStore(...)}.</li>
 *   <li>Does not poll SA for post-close state — the local store is
 *       fire-and-mark; query {@code mppServer.status(channelId)} for
 *       authoritative on-chain state.</li>
 * </ul>
 */
public final class MppPaymentInterceptor implements HandlerInterceptor {

    private static final Logger log = LoggerFactory.getLogger(MppPaymentInterceptor.class);

    public static final String AUTHORIZATION_PREFIX = "Payment ";
    public static final String CHANNEL_ID_HEADER    = "X-Channel-Id";
    public static final String SPENT_HEADER         = "X-Spent";
    public static final String UNITS_HEADER         = "X-Units";
    public static final String PAYMENT_RECEIPT_HEADER = "Payment-Receipt";

    /** Stash key for {@link Context} between preHandle and postHandle. */
    static final String ATTR_CTX = "mpp.payment.context";

    private final MppServer mppServer;
    private final MppRouteConfig routes;
    private final ObjectMapper mapper = new ObjectMapper();
    private final Base64UrlJson base64UrlJson = new Base64UrlJson(mapper);

    private Function<HttpServletRequest, String> channelIdResolver = defaultChannelIdResolver();
    private BiFunction<HttpServletRequest, MppRouteConfig.Entry, ProtectedRequestResult>
            onProtectedRequest = (req, entry) -> ProtectedRequestResult.proceed();

    public MppPaymentInterceptor(MppServer mppServer, MppRouteConfig routes) {
        this.mppServer = Objects.requireNonNull(mppServer, "mppServer");
        this.routes    = Objects.requireNonNull(routes, "routes");
    }

    public static MppPaymentInterceptor create(MppServer mppServer, MppRouteConfig routes) {
        return new MppPaymentInterceptor(mppServer, routes);
    }

    /** Override channelId resolution (default: {@code X-Channel-Id} header). */
    public MppPaymentInterceptor channelIdResolver(Function<HttpServletRequest, String> resolver) {
        this.channelIdResolver = Objects.requireNonNull(resolver, "channelIdResolver");
        return this;
    }

    /**
     * Hook for the merchant to short-circuit MPP billing (e.g. read
     * {@code paymentExempt} request attribute set by an upstream auth
     * filter). Same {@link ProtectedRequestResult} contract as x402's
     * {@code PaymentInterceptor}.
     */
    public MppPaymentInterceptor onProtectedRequest(
            BiFunction<HttpServletRequest, MppRouteConfig.Entry, ProtectedRequestResult> hook) {
        this.onProtectedRequest = Objects.requireNonNull(hook, "hook");
        return this;
    }

    /** Expose the underlying server for additional configuration / observability. */
    public MppServer mppServer() { return mppServer; }
    public MppRouteConfig routes() { return routes; }

    /** Returns true if the configured route table covers this URI. */
    public boolean matches(HttpServletRequest request) {
        return routes.match(request.getRequestURI()) != null;
    }

    // ── HandlerInterceptor ────────────────────────────────────────────────────

    @Override
    public boolean preHandle(HttpServletRequest request,
                             HttpServletResponse response,
                             Object handler) throws Exception {

        MppRouteConfig.Entry entry = routes.match(request.getRequestURI());
        if (entry == null) {
            return true;                       // not an MPP route → pass through
        }

        switch (entry.kind()) {
            case SESSION_MANAGE -> {
                handleSessionManage(request, response, entry);
                return false;                  // terminal — handler does not run
            }
            case RESOURCE -> {
                ProtectedRequestResult res;
                try {
                    res = onProtectedRequest.apply(request, entry);
                } catch (RuntimeException e) {
                    log.warn("[mpp-int] onProtectedRequest hook threw, defaulting to proceed", e);
                    res = ProtectedRequestResult.proceed();
                }

                switch (res.decision) {
                    case GRANT_ACCESS -> {
                        // Free-tier / exempt — no MPP deduct, no channelId required.
                        return true;
                    }
                    case ABORT -> {
                        write(response, 403, "application/json",
                            "{\"error\":\"" + esc(res.reason == null ? "forbidden" : res.reason) + "\"}");
                        return false;
                    }
                    case PROCEED -> {
                        // fall through — bind channelId for postHandle deduct
                    }
                }

                String channelId = channelIdResolver.apply(request);
                if (channelId == null || channelId.isBlank()) {
                    writeProblem(response, 400, "https://paymentauth.org/problems/bad-request",
                        "Bad Request",
                        "missing channelId (set " + CHANNEL_ID_HEADER + " header or open a session via "
                            + "the session-manage endpoint)");
                    return false;
                }
                request.setAttribute(ATTR_CTX, new Context(channelId, entry));
                return true;
            }
            default -> {
                return true;
            }
        }
    }

    @Override
    public void postHandle(HttpServletRequest request,
                           HttpServletResponse response,
                           Object handler,
                           ModelAndView modelAndView) throws Exception {

        Context ctx = (Context) request.getAttribute(ATTR_CTX);
        if (ctx == null) return;
        if (response.getStatus() >= 400) return;       // handler failed — don't bill
        if (response.isCommitted())       return;       // already flushed (rare)

        // Resolve the option whose currency matches the channel's pinned token.
        // The channel was opened against one specific ERC-20; every subsequent
        // billed call charges that token at its option's price.
        SessionStore.Channel channel = mppServer.sessionStore().load(ctx.channelId).orElse(null);
        if (channel == null) {
            log.warn("[mpp-int] resource {} channel not found channelId={}",
                request.getRequestURI(), ctx.channelId);
            if (!response.isCommitted()) {
                response.reset();
                writeProblem(response, 400, "https://paymentauth.org/problems/bad-request",
                    "Bad Request", "channel not found: " + ctx.channelId);
            }
            return;
        }
        MppRouteConfig.Option opt = ctx.entry.findOptionByToken(channel.token());
        if (opt == null) {
            log.warn("[mpp-int] resource {} no matching option for channel token={} channelId={}",
                request.getRequestURI(), channel.token(), ctx.channelId);
            if (!response.isCommitted()) {
                response.reset();
                writeProblem(response, 400, "https://paymentauth.org/problems/bad-request",
                    "Bad Request",
                    "channel token " + channel.token() + " is not in this route's options");
            }
            return;
        }

        try {
            SessionStore.DeductResult d = mppServer.deductFromChannel(ctx.channelId, opt.price());
            response.setHeader(SPENT_HEADER, d.spent().toString());
            response.setHeader(UNITS_HEADER, String.valueOf(d.units()));
            response.setHeader(PAYMENT_RECEIPT_HEADER,
                buildReceiptB64(ctx.channelId, opt, d));
        } catch (InsufficientBalanceError e) {
            log.info("[mpp-int] resource {} exhausted channelId={}: {}",
                request.getRequestURI(), ctx.channelId, e.getMessage());
            if (!response.isCommitted()) {
                response.reset();
                emitMultiOptionChallenge(response, ctx.entry);
                writeProblem(response, e.status(), e.type(), e.title(), e.getMessage());
            }
        } catch (MppError e) {
            log.warn("[mpp-int] resource {} deduct failed channelId={} code={}: {}",
                request.getRequestURI(), ctx.channelId, e.code(), e.getMessage());
            if (!response.isCommitted()) {
                response.reset();
                writeProblem(response, e.status(), e.type(), e.title(), e.getMessage());
            }
        }
    }

    /** Emit one {@code WWW-Authenticate: Payment …} line per option. */
    private void emitMultiOptionChallenge(HttpServletResponse response,
                                          MppRouteConfig.Entry entry) {
        for (SessionRequest sr : entry.buildSessionRequests()) {
            Challenge ch = mppServer.request(entry.realm(), Intent.SESSION, sr);
            response.addHeader("WWW-Authenticate", MppAdapter.serializeWwwAuth(ch));
        }
    }

    // ── Terminal: /session/manage ─────────────────────────────────────────────

    private void handleSessionManage(HttpServletRequest request, HttpServletResponse response,
                                     MppRouteConfig.Entry entry) throws IOException {
        Credential credential;
        try {
            credential = decodeAuthHeader(request);
        } catch (RuntimeException e) {
            writeProblem(response, 400, "https://paymentauth.org/problems/bad-request",
                "Bad Request", "malformed Authorization: Payment header: " + e.getMessage());
            return;
        }

        // Derive the SessionRequest from the buyer's echoed Challenge.
        // HMAC-verify provenance first to reject forged option claims —
        // credential-supplied SessionRequest is otherwise buyer-controlled.
        SessionRequest sessionRequest;
        try {
            sessionRequest = decodeEchoedSessionRequest(credential, entry);
        } catch (RuntimeException e) {
            writeProblem(response, 400, "https://paymentauth.org/problems/bad-request",
                "Bad Request", e.getMessage());
            return;
        }

        try {
            SessionResult result = mppServer.evmSessionMethod().verifySession(credential, sessionRequest);
            response.setStatus(200);
            response.setContentType("application/json");
            response.getWriter().write(mapper.writeValueAsString(toResponseBody(result)));
        } catch (MppError e) {
            log.info("[mpp-int] session/manage rejected: code={} detail={}", e.code(), e.getMessage());
            writeProblem(response, e.status(), e.type(), e.title(), e.getMessage());
        }
    }

    /**
     * Pick the canonical {@link SessionRequest} for the verifySession call.
     *
     * <p>For {@code open} and {@code voucher}, decodes the echoed
     * {@code credential.challenge} (HMAC-verified) — the buyer's choice of
     * option. For {@code topUp} / {@code close} the challenge is null (these
     * are merchant- or channel-keyed, not 402-driven), so we fall back to the
     * channel-pinned option to keep verifySession's signature uniform.
     */
    private SessionRequest decodeEchoedSessionRequest(Credential credential,
                                                      MppRouteConfig.Entry entry) {
        Challenge ch = credential.challenge();
        if (ch != null) {
            // T1-3 / T1-5: HMAC verify AND expires check — fail fast before decoding the request
            // payload from the (untrusted) challenge. The façade does verifyAlive again as
            // defence-in-depth (for callers that bypass this interceptor), but rejecting here
            // saves the decode cost on forged or replayed challenges.
            if (!mppServer.challengeBuilder().verifyAlive(ch)) {
                throw new IllegalArgumentException(
                    "echoed challenge failed HMAC verify or has expired "
                        + "(not issued by this seller, or tampered, or stale)");
            }
            String requestB64 = ch.request();
            if (requestB64 == null || requestB64.isBlank()) {
                throw new IllegalArgumentException("echoed challenge has no request payload");
            }
            try {
                return base64UrlJson.decode(requestB64, SessionRequest.class);
            } catch (RuntimeException e) {
                throw new IllegalArgumentException(
                    "malformed SessionRequest inside echoed challenge: " + e.getMessage(), e);
            }
        }
        // No challenge → topUp/close. Use the first option's SessionRequest as
        // a sentinel; EvmSessionMethod.handleTopUp / handleClose ignore it.
        return entry.options().get(0).toSessionRequest(entry.description());
    }

    // ── Helpers (mostly mirrored from MppAdapter) ────────────────────────────

    private Credential decodeAuthHeader(HttpServletRequest request) {
        String h = request.getHeader("Authorization");
        if (h == null || !h.toLowerCase(Locale.ROOT)
                .startsWith(AUTHORIZATION_PREFIX.toLowerCase(Locale.ROOT))) {
            throw new IllegalArgumentException("missing or non-Payment Authorization header");
        }
        String b64 = h.substring(AUTHORIZATION_PREFIX.length()).trim();
        return base64UrlJson.decode(b64, Credential.class);
    }

    private static Function<HttpServletRequest, String> defaultChannelIdResolver() {
        return req -> {
            String h = req.getHeader(CHANNEL_ID_HEADER);
            return (h != null && !h.isBlank()) ? h.trim() : null;
        };
    }

    private Map<String, Object> toResponseBody(SessionResult result) {
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("action", result.action());
        body.put("channelId", result.channelId());
        if (result instanceof SessionResult.OpenResult o) {
            Receipt.SessionReceipt r = o.receipt();
            if (r.deposit() != null) body.put("deposit", r.deposit());
            if (r.reference() != null) body.put("reference", r.reference());
        } else if (result instanceof SessionResult.TopUpResult t) {
            Receipt.SessionReceipt r = t.receipt();
            if (r.deposit() != null) body.put("deposit", r.deposit());
            if (r.reference() != null) body.put("reference", r.reference());
        } else if (result instanceof SessionResult.VoucherResult v) {
            body.put("acceptedCumulativeAmount", v.acceptedCumulativeAmount().toString());
            body.put("idempotent", v.idempotent());
            body.put("spent", v.spent().toString());
            body.put("units", v.units());
        } else if (result instanceof SessionResult.CloseResult c) {
            Receipt.SessionReceipt r = c.receipt();
            if (r.reference() != null) body.put("reference", r.reference());
        }
        return body;
    }

    private String buildReceiptB64(String channelId, MppRouteConfig.Option opt,
                                   SessionStore.DeductResult d) {
        Map<String, Object> receipt = new LinkedHashMap<>();
        receipt.put("method", "evm");
        receipt.put("intent", "session");
        receipt.put("status", "success");
        receipt.put("channelId", channelId);
        receipt.put("spent", d.spent().toString());
        receipt.put("units", d.units());
        receipt.put("amount", opt.price().toString());
        receipt.put("currency", opt.currency());
        return base64UrlJson.encode(receipt);
    }

    private void writeProblem(HttpServletResponse resp, int status, String type, String title, String detail)
            throws IOException {
        if (resp.isCommitted()) return;
        resp.setStatus(status);
        resp.setContentType("application/problem+json");
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("type", type);
        body.put("title", title);
        body.put("status", status);
        body.put("detail", detail);
        resp.getWriter().write(mapper.writeValueAsString(body));
    }

    private static void write(HttpServletResponse resp, int status, String contentType, String body)
            throws IOException {
        if (resp.isCommitted()) return;
        resp.setStatus(status);
        resp.setContentType(contentType);
        resp.getWriter().write(body);
    }

    private static String esc(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    /** Per-request stash carried from {@link #preHandle} to {@link #postHandle}. */
    static final class Context {
        final String channelId;
        final MppRouteConfig.Entry entry;
        Context(String channelId, MppRouteConfig.Entry entry) {
            this.channelId = channelId;
            this.entry = entry;
        }
    }
}
