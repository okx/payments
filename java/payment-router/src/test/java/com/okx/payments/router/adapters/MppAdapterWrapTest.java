package com.okx.payments.router.adapters;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.seller.InMemorySessionStore;
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.seller.PrivateKeyPayeeAuthSigner;
import com.okx.payments.mpp.seller.SessionStore;
import com.okx.payments.mpp.server.MppRouteConfig;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import javax.servlet.FilterChain;
import javax.servlet.http.HttpServletRequest;
import javax.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.io.PrintWriter;
import java.io.StringWriter;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.util.Base64;

import static org.assertj.core.api.Assertions.assertThat;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.anyInt;
import static org.mockito.ArgumentMatchers.anyString;
import static org.mockito.ArgumentMatchers.eq;
import static org.mockito.Mockito.atLeastOnce;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * Verifies the M11 wrap pattern in {@link MppAdapter#handle}:
 *  - SESSION_MANAGE: terminal — decode credential, dispatch via EvmSessionMethod, write JSON, no chain.
 *  - RESOURCE     : pre+chain+post — channelId resolved, chain runs, deduct after success, headers injected.
 *  - 70015 path   : reset response + emit 402 with WWW-Authenticate.
 */
class MppAdapterWrapTest {

    private static final String CHANNEL_ID = "0x" + "ab".repeat(32);
    private static final String TOKEN = "0x74b7F1633b89720027F6196A17a631aC6dE26d22";

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();
    private final BigInteger uint256Max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);
    private final ObjectMapper mapper = new ObjectMapper();
    private final Base64UrlJson b64j = new Base64UrlJson(mapper);

    private final ECKeyPair payerKey = ECKeyPair.create(BigInteger.valueOf(7));
    private final String payerAddr = "0x" + Keys.getAddress(payerKey);
    private final String payeeAddr = "0x" + Keys.getAddress(ECKeyPair.create(BigInteger.valueOf(13)));

    private SaApiClient sa;
    private MppServer server;
    private MppAdapter adapter;

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
        adapter = new MppAdapter(server);
    }

    private MppRouteConfig.Entry sessionManageEntry(BigInteger price) {
        return new MppRouteConfig.Entry(
            MppRouteConfig.Kind.SESSION_MANAGE,
            "demo.realm",
            "demo",
            java.util.List.of(MppRouteConfig.Option.of(
                price, TOKEN, payeeAddr,
                new SessionMethodDetails(domain.chainId(), domain.escrowAddress(), null, "0", null))));
    }

    private MppRouteConfig.Entry resourceEntry(BigInteger price) {
        return new MppRouteConfig.Entry(
            MppRouteConfig.Kind.RESOURCE,
            "demo.realm",
            "demo",
            java.util.List.of(MppRouteConfig.Option.of(
                price, TOKEN, payeeAddr,
                new SessionMethodDetails(domain.chainId(), domain.escrowAddress(), null, "0", null))));
    }

    @Test
    void session_manage_terminal_dispatches_voucher_writes_json_no_chain() throws Exception {
        // T1-6: voucher acceptance is now decoupled from billing — handleVoucher only validates
        // and stores the voucher. Billing happens explicitly via deductFromChannel on subsequent
        // billable requests (e.g. resource_path_pre_chain_post_deducts_and_injects_headers
        // exercises that path). For the session-manage terminal we just confirm the voucher was
        // accepted and the JSON shape reports spent=0/units=0 (no implicit deduction).
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
        when(req.getHeader("Authorization")).thenReturn(authHeader);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));
        FilterChain chain = mock(FilterChain.class);

        adapter.handle(req, resp, chain, sessionManageEntry(BigInteger.valueOf(100)));

        verify(resp).setStatus(200);
        verify(resp).setContentType("application/json");
        verify(chain, never()).doFilter(any(), any());
        assertThat(sw.toString()).contains("\"action\":\"voucher\"");
        assertThat(sw.toString()).contains("\"channelId\":\"" + CHANNEL_ID + "\"");
        // T1-6: voucher acceptance no longer auto-deducts — spent/units stay at 0 until
        // billable resource calls invoke deductFromChannel explicitly.
        assertThat(sw.toString()).contains("\"spent\":\"0\"");
        assertThat(sw.toString()).contains("\"units\":0");
        // Voucher itself was accepted — store reflects new highest accepted cumulative.
        assertThat(server.sessionStore().load(CHANNEL_ID).orElseThrow().lastAccepted())
            .isEqualTo(cum);
    }

    @Test
    void resource_path_pre_chain_post_deducts_and_injects_headers() throws Exception {
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(1_000), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getHeader(MppAdapter.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(200);
        FilterChain chain = mock(FilterChain.class);

        adapter.handle(req, resp, chain, resourceEntry(BigInteger.valueOf(100)));

        // chain ran ONCE before deduct
        verify(chain, times(1)).doFilter(req, resp);
        // post-deduct headers set
        verify(resp).setHeader(eq(MppAdapter.SPENT_HEADER), eq("100"));
        verify(resp).setHeader(eq(MppAdapter.UNITS_HEADER), eq("1"));
        verify(resp).setHeader(eq(MppAdapter.PAYMENT_RECEIPT_HEADER), anyString());

        // Channel state advanced
        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(100);
        assertThat(ch.units()).isEqualTo(1);
    }

    @Test
    void resource_path_with_failed_chain_does_not_deduct() throws Exception {
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(1_000), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        when(req.getHeader(MppAdapter.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(500);   // merchant handler failed
        FilterChain chain = mock(FilterChain.class);

        adapter.handle(req, resp, chain, resourceEntry(BigInteger.valueOf(100)));

        verify(chain, times(1)).doFilter(req, resp);
        // No deduct headers — buyer not billed for failed request
        verify(resp, never()).setHeader(eq(MppAdapter.SPENT_HEADER), anyString());
        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(BigInteger.ZERO);
        assertThat(ch.units()).isEqualTo(0);
    }

    @Test
    void resource_path_missing_channel_id_emits_400() throws Exception {
        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));
        when(req.getHeader(MppAdapter.CHANNEL_ID_HEADER)).thenReturn(null);
        FilterChain chain = mock(FilterChain.class);

        adapter.handle(req, resp, chain, resourceEntry(BigInteger.valueOf(100)));

        verify(resp).setStatus(400);
        verify(chain, never()).doFilter(any(), any());
        assertThat(sw.toString()).contains("missing channelId");
    }

    @Test
    void resource_70015_resets_response_and_emits_402_with_wwwauth() throws Exception {
        // Channel exhausted: lastAccepted=100, spent=100 → available=0 < price 50
        server.sessionStore().put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(100), new byte[65],
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN,
            BigInteger.valueOf(100), 1L));

        HttpServletRequest req = mock(HttpServletRequest.class);
        HttpServletResponse resp = mock(HttpServletResponse.class);
        StringWriter sw = new StringWriter();
        when(resp.getWriter()).thenReturn(new PrintWriter(sw));
        when(req.getHeader(MppAdapter.CHANNEL_ID_HEADER)).thenReturn(CHANNEL_ID);
        when(resp.getStatus()).thenReturn(200);
        when(resp.isCommitted()).thenReturn(false);
        FilterChain chain = mock(FilterChain.class);

        adapter.handle(req, resp, chain, resourceEntry(BigInteger.valueOf(50)));

        // Chain still ran (we deduct after)
        verify(chain, times(1)).doFilter(req, resp);
        // 402 emitted, response was reset
        verify(resp, atLeastOnce()).reset();
        verify(resp).setStatus(402);
        verify(resp).addHeader(eq("WWW-Authenticate"), anyString());
        assertThat(sw.toString()).contains("insufficient-balance");
    }

    @Test
    void www_auth_serializer_handles_optional_fields() {
        Challenge ch = new Challenge("x", "r", Method.EVM, Intent.SESSION,
            "REQ", "EXPIRES", "Test", null, null);
        String s = MppAdapter.serializeWwwAuth(ch);
        assertThat(s).startsWith("Payment ");
        assertThat(s).contains("description=\"Test\"");
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
