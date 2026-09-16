package com.okx.payments.router;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.seller.InMemorySessionStore;
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.seller.PrivateKeyPayeeAuthSigner;
import com.okx.payments.mpp.seller.SessionStore;
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import com.okx.x402.server.PaymentHooks.ProtectedRequestResult;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.HashMap;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.anyInt;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.atLeastOnce;
import static org.mockito.Mockito.doAnswer;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * Verifies {@link MppPaymentInterceptor}'s preHandle/postHandle contract:
 *  - URI not in routes → preHandle returns true (pass-through)
 *  - SESSION_MANAGE   → terminal: write JSON, return false
 *  - RESOURCE missing X-Channel-Id → 400 + return false
 *  - RESOURCE happy path → preHandle stashes context, postHandle deducts + injects headers
 *  - RESOURCE failed handler (status >= 400) → no deduct
 *  - RESOURCE 70015 → reset response + 402 with WWW-Authenticate
 *  - onProtectedRequest GRANT_ACCESS → preHandle returns true, postHandle skips
 *  - onProtectedRequest ABORT → 403 + return false
 */
class MppPaymentInterceptorTest {

    private static final String CHANNEL_ID = "0x" + "ab".repeat(32);
    private static final String TOKEN = "0x74b7F1633b89720027F6196A17a631aC6dE26d22";
    private static final String RESOURCE_URI = "/api/v6/dex/market/price";
    private static final String SESSION_MGT_URI = "/api/v6/dex/market/session/manage";

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();
    private final BigInteger uint256Max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);
    private final ObjectMapper mapper = new ObjectMapper();
    private final Base64UrlJson b64j = new Base64UrlJson(mapper);

    private final ECKeyPair payerKey = ECKeyPair.create(BigInteger.valueOf(7));
    private final String payerAddr = "0x" + Keys.getAddress(payerKey);
    private final String payeeAddr = "0x" + Keys.getAddress(ECKeyPair.create(BigInteger.valueOf(13)));

    private SaApiClient sa;
    private MppServer server;
    private MppRouteConfig routes;
    private MppPaymentInterceptor interceptor;

    @BeforeEach
    void setup() {
        sa = mock(SaApiClient.class);
        server = MppServer.builder()
            .saApiClient(sa)
            .challengeSecretKey(new byte[]{1, 2, 3, 4, 5, 6, 7, 8})
            .payeeAuthSigner(PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16)))
            .domain(domain)
            .sessionStore(new InMemorySessionStore())
            .nonceProvider((p, c) -> BigInteger.valueOf(42))
            .deadlineDefault(uint256Max)
            .build();
        SessionMethodDetails sd = new SessionMethodDetails(domain.chainId(), domain.escrowAddress(),
            null, "0", null);
        routes = MppRouteConfig.builder()
            .sessionManage(SESSION_MGT_URI, "demo.realm",
                java.util.List.of(MppRouteConfig.Option.of(
                    BigInteger.ZERO, TOKEN, payeeAddr, sd)))
            .resource(RESOURCE_URI, "demo.realm",
                java.util.List.of(MppRouteConfig.Option.of(
                    BigInteger.valueOf(100), TOKEN, payeeAddr, sd)));
        interceptor = new MppPaymentInterceptor(server, routes);
    }

    // ── 1. URI outside routes → pass-through ───────────────────────────────────

    @Test
    void uri_not_in_routes_passes_through() throws Exception {
        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getRequestURI()).thenReturn("/health");

        boolean cont = interceptor.preHandle(req, resp, null);

        assertThat(cont).isTrue();
        verify(resp, never()).setStatus(anyInt());
    }

    // ── 2. SESSION_MANAGE terminal ─────────────────────────────────────────────

    @Test
    void session_manage_terminal_writes_json_returns_false() throws Exception {
        // Pre-seed channel with deposit=10000
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        BigInteger cum = BigInteger.valueOf(500);
        String sigHex = "0x" + bytesToHex(signVoucher(cum));
        String voucherBody = "{\"action\":\"voucher\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"" + cum + "\",\"signature\":\"" + sigHex + "\"}";
        String credentialJson = "{\"payload\":" + voucherBody + "}";
        String authHeader = "Payment " + Base64.getUrlEncoder().withoutPadding()
            .encodeToString(credentialJson.getBytes(StandardCharsets.UTF_8));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(req.getRequestURI()).thenReturn(SESSION_MGT_URI);
        when(req.getHeader("Authorization")).thenReturn(authHeader);
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));

        boolean cont = interceptor.preHandle(req, resp, null);

        assertThat(cont).isFalse();
        verify(resp).setStatus(200);
        verify(resp).setContentType("application/json");
        assertThat(sw.toString()).contains("\"action\":\"voucher\"");
        assertThat(sw.toString()).contains("\"channelId\":\"" + CHANNEL_ID + "\"");
        // price=0 on session-manage → no implicit deduct
        assertThat(sw.toString()).contains("\"spent\":\"0\"");
        assertThat(sw.toString()).contains("\"units\":0");
    }

    // ── 3. RESOURCE happy path ────────────────────────────────────────────────

    @Test
    void resource_happy_path_stashes_context_and_postHandle_deducts() throws Exception {
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(1_000), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        wireAttrs(req);
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(req.getHeader(MppPaymentInterceptor.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(200);

        boolean cont = interceptor.preHandle(req, resp, null);
        assertThat(cont).isTrue();

        // Spring would invoke the controller here; then it calls postHandle.
        interceptor.postHandle(req, resp, null, null);

        verify(resp).setHeader(eq(MppPaymentInterceptor.SPENT_HEADER), eq("100"));
        verify(resp).setHeader(eq(MppPaymentInterceptor.UNITS_HEADER), eq("1"));
        verify(resp).setHeader(eq(MppPaymentInterceptor.PAYMENT_RECEIPT_HEADER), anyString());

        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(100);
        assertThat(ch.units()).isEqualTo(1);
    }

    // ── 4. RESOURCE missing X-Channel-Id ──────────────────────────────────────

    @Test
    void resource_missing_channel_id_writes_400_and_returns_false() throws Exception {
        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(req.getHeader(MppPaymentInterceptor.CHANNEL_ID_HEADER)).thenReturn(null);
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));

        boolean cont = interceptor.preHandle(req, resp, null);

        assertThat(cont).isFalse();
        verify(resp).setStatus(400);
        assertThat(sw.toString()).contains("missing channelId");
    }

    // ── 5. RESOURCE failed handler (response.status >= 400) — no deduct ────────

    @Test
    void resource_failed_handler_does_not_deduct() throws Exception {
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(1_000), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        wireAttrs(req);
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(req.getHeader(MppPaymentInterceptor.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(500);

        interceptor.preHandle(req, resp, null);
        interceptor.postHandle(req, resp, null, null);

        verify(resp, never()).setHeader(eq(MppPaymentInterceptor.SPENT_HEADER), anyString());
        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(BigInteger.ZERO);
    }

    // ── 6. RESOURCE 70015 path → reset + 402 + WWW-Authenticate ───────────────

    @Test
    void resource_insufficient_balance_resets_and_emits_402() throws Exception {
        // available=0 (lastAccepted=100, spent=100); price=100 → 70015
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(100), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN,
            BigInteger.valueOf(100), 1L));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        wireAttrs(req);
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(req.getHeader(MppPaymentInterceptor.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(200);
        when(resp.isCommitted()).thenReturn(false);
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));

        interceptor.preHandle(req, resp, null);
        interceptor.postHandle(req, resp, null, null);

        verify(resp, atLeastOnce()).reset();
        verify(resp).setStatus(402);
        verify(resp).addHeader(eq("WWW-Authenticate"), anyString());
        assertThat(sw.toString()).contains("insufficient-balance");
    }

    // ── 7. onProtectedRequest GRANT_ACCESS → no deduct ────────────────────────

    @Test
    void onProtectedRequest_grant_skips_deduct_and_returns_true() throws Exception {
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(1_000), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        interceptor.onProtectedRequest((req, entry) -> ProtectedRequestResult.grantAccess());

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        wireAttrs(req);
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(req.getHeader(MppPaymentInterceptor.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(200);

        boolean cont = interceptor.preHandle(req, resp, null);
        interceptor.postHandle(req, resp, null, null);

        assertThat(cont).isTrue();
        verify(resp, never()).setHeader(eq(MppPaymentInterceptor.SPENT_HEADER), anyString());
        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(BigInteger.ZERO);
    }

    // ── 8. onProtectedRequest ABORT → 403 + return false ──────────────────────

    @Test
    void onProtectedRequest_abort_writes_403() throws Exception {
        interceptor.onProtectedRequest((req, entry) -> ProtectedRequestResult.abort("not-allowed"));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(req.getRequestURI()).thenReturn(RESOURCE_URI);
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));

        boolean cont = interceptor.preHandle(req, resp, null);

        assertThat(cont).isFalse();
        verify(resp).setStatus(403);
        assertThat(sw.toString()).contains("not-allowed");
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    /**
     * Wire setAttribute/getAttribute round-trip on a mock — Mockito.mock does
     * not correlate them by default, so the Context stashed in preHandle
     * returns null in postHandle.
     */
    private static void wireAttrs(HttpServletRequest req) {
        Map<String, Object> attrs = new HashMap<>();
        doAnswer(inv -> { attrs.put(inv.getArgument(0), inv.getArgument(1)); return null; })
            .when(req).setAttribute(anyString(), any());
        when(req.getAttribute(anyString())).thenAnswer(inv -> attrs.get(inv.getArgument(0)));
    }

    private byte[] signVoucher(BigInteger cum) {
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.voucherStructHash(CHANNEL_ID, cum));
        return Eip712Signer.sign(digest, payerKey);
    }

    private static String bytesToHex(byte[] b) {
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) sb.append(String.format("%02x", x));
        return sb.toString();
    }
}
