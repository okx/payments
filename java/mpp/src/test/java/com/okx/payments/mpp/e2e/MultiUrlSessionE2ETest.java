package com.okx.payments.mpp.e2e;

import com.okx.payments.mpp.errors.InsufficientBalanceError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.nonce.NonceProvider;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.seller.InMemorySessionStore;
import com.okx.payments.mpp.seller.MppServer;
import com.okx.payments.mpp.seller.PayeeAuthSigner;
import com.okx.payments.mpp.seller.PrivateKeyPayeeAuthSigner;
import com.okx.payments.mpp.seller.SessionHandler;
import com.okx.payments.mpp.seller.SessionStore;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.math.BigInteger;
import java.time.Clock;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/**
 * End-to-end multi-URL session flow — exercises the full Rust-aligned cycle:
 *
 * <pre>
 * (1) handleOpen           → channel persisted with deposit
 * (2) acceptVoucher cum=A  → first off-band voucher
 * (3) deductFromChannel    → /resourceA (small)
 * (4) deductFromChannel    → /resourceB (medium)
 * (5) deductFromChannel    → /resourceC (large)
 * (6) deductFromChannel    → /resourceD (would exceed) → 70015
 * (7) acceptVoucher cum=B  → client raises voucher in response to 402
 * (8) deductFromChannel    → /resourceD now fits
 * (9) settle               → on-chain checkpoint
 * (10) close               → final settle / channel teardown
 * </pre>
 *
 * <p>This is the canonical integrator scenario. SA is mocked at the API boundary;
 * everything else (voucher 712 sign/verify, monotonicity, deduct atomicity, settle
 * payee-EOA sign, close-with-voucher) runs through the real SDK code paths.
 */
class MultiUrlSessionE2ETest {

    private static final String CHANNEL_ID = "0x" + "ab".repeat(32);

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();
    private final BigInteger uint256Max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);

    private final ECKeyPair payerKey = ECKeyPair.create(BigInteger.valueOf(7));
    private final String payerAddr = "0x" + Keys.getAddress(payerKey);

    private final ECKeyPair payeeKey = ECKeyPair.create(BigInteger.valueOf(13));
    private final String payeeAddr = "0x" + Keys.getAddress(payeeKey);

    private SaApiClient sa;
    private MppServer server;

    @BeforeEach
    void setup() {
        sa = mock(SaApiClient.class);

        // Wire up MppServer with the real handler stack but a mocked SA boundary.
        server = MppServer.builder()
            .saApiClient(sa)
            .challengeSecretKey(new byte[]{1, 2, 3, 4, 5, 6, 7, 8})
            .payeeAuthSigner(PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16)))
            .domain(domain)
            .sessionStore(new InMemorySessionStore())
            .nonceProvider((p, c) -> BigInteger.valueOf(42))
            .deadlineDefault(uint256Max)
            .minVoucherDelta(BigInteger.ZERO)
            .clock(Clock.systemUTC())
            .build();
    }

    @Test
    @DisplayName("Open → voucher → multi-URL deduct → 70015 → topUp voucher → continue → settle → close")
    void full_multi_url_session_lifecycle() {
        // (1) open — SA returns channel + deposit=10_000
        Receipt.SessionReceipt openResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "10000", domain.chainId(), "0xopentx");
        when(sa.sessionOpen(any())).thenReturn(openResp);

        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            payerAddr, payeeAddr,
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");
        Receipt.SessionReceipt openReceipt = server.sessionHandler().handleOpen("openBody", init);
        assertThat(openReceipt.channelId()).isEqualTo(CHANNEL_ID);

        SessionStore.Channel ch0 = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch0.deposit()).isEqualTo(10_000);
        assertThat(ch0.lastAccepted()).isEqualTo(BigInteger.ZERO);
        assertThat(ch0.spent()).isEqualTo(BigInteger.ZERO);
        assertThat(ch0.units()).isEqualTo(0L);

        // (2) accept first voucher cum=1000
        BigInteger cum1 = BigInteger.valueOf(1_000);
        server.acceptVoucher(CHANNEL_ID, cum1, signVoucher(cum1));

        // (3)–(5) deduct for three different URLs
        SessionStore.DeductResult dA = server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(100));
        SessionStore.DeductResult dB = server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(300));
        SessionStore.DeductResult dC = server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(500));

        assertThat(dA.spent()).isEqualTo(100);
        assertThat(dA.units()).isEqualTo(1);
        assertThat(dB.spent()).isEqualTo(400);
        assertThat(dB.units()).isEqualTo(2);
        assertThat(dC.spent()).isEqualTo(900);
        assertThat(dC.units()).isEqualTo(3);

        // available = 1000 - 900 = 100
        SessionStore.Channel chMid = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(chMid.spent()).isEqualTo(900);
        assertThat(chMid.lastAccepted()).isEqualTo(1_000);

        // (6) /resourceD costs 200 — only 100 available, so 70015
        assertThatThrownBy(() ->
            server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(200)))
            .isInstanceOf(InsufficientBalanceError.class)
            .hasMessageContaining("requested 200")
            .hasMessageContaining("available 100");

        // Client should now sign a higher voucher; verify spent unchanged after the 402.
        SessionStore.Channel chAfter402 = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(chAfter402.spent()).isEqualTo(900);
        assertThat(chAfter402.units()).isEqualTo(3);   // failed deduct didn't increment

        // (7) accept higher voucher cum=2000
        BigInteger cum2 = BigInteger.valueOf(2_000);
        server.acceptVoucher(CHANNEL_ID, cum2, signVoucher(cum2));

        // (8) /resourceD now fits — available = 2000 - 900 = 1100, deduct 200
        SessionStore.DeductResult dD = server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(200));
        assertThat(dD.spent()).isEqualTo(1_100);
        assertThat(dD.units()).isEqualTo(4);

        // (9) settle to on-chain
        Receipt.SessionReceipt settleResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "10000", domain.chainId(), "0xsettletx");
        when(sa.sessionSettle(any())).thenReturn(settleResp);

        Receipt.SessionReceipt settled = server.settle(CHANNEL_ID);
        assertThat(settled.reference()).isEqualTo("0xsettletx");
        SessionStore.Channel chSettled = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(chSettled.settledOnChain()).isEqualTo(cum2);   // accepted voucher cum
        verify(sa, times(1)).sessionSettle(any());

        // (10) close — voucher waiver path because settledOnChain == lastAccepted
        Receipt.SessionReceipt closeResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "10000", domain.chainId(), "0xclosetx");
        when(sa.sessionClose(any())).thenReturn(closeResp);

        Receipt.SessionReceipt closed = server.close(CHANNEL_ID);
        assertThat(closed.reference()).isEqualTo("0xclosetx");
        SessionStore.Channel chClosed = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(chClosed.status()).isEqualTo(SessionStore.ChannelStatus.CLOSING);
    }

    @Test
    @DisplayName("Strict topUp parity — SA must return authoritative deposit; no client-side fallback")
    void strict_topup_rejects_missing_deposit() {
        seedChannel(BigInteger.valueOf(5_000));

        Receipt.SessionReceipt topupResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, null, domain.chainId(), "0xtopup");   // ← missing deposit
        when(sa.sessionTopUp(any())).thenReturn(topupResp);

        assertThatThrownBy(() -> server.sessionHandler().handleTopUp("body"))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("missing required deposit");
    }

    @Test
    @DisplayName("Strict topUp parity — happy path updates local deposit from SA truth")
    void strict_topup_updates_deposit_from_sa() {
        seedChannel(BigInteger.valueOf(5_000));

        Receipt.SessionReceipt topupResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "12000", domain.chainId(), "0xtopup");
        when(sa.sessionTopUp(any())).thenReturn(topupResp);

        server.sessionHandler().handleTopUp("body");
        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.deposit()).isEqualTo(BigInteger.valueOf(12_000));
    }

    @Test
    @DisplayName("Idempotent voucher replay — no-op deduct, deduct API still operates independently")
    void replay_voucher_does_not_double_deduct() {
        seedChannel(BigInteger.valueOf(10_000));

        BigInteger cum = BigInteger.valueOf(500);
        byte[] sig = signVoucher(cum);

        // First accept
        SessionHandler.VoucherAck a1 = server.acceptVoucher(CHANNEL_ID, cum, sig);
        assertThat(a1.idempotent()).isFalse();
        server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(100));

        // Replay same voucher — must be idempotent at acceptance level (no spent change)
        SessionHandler.VoucherAck a2 = server.acceptVoucher(CHANNEL_ID, cum, sig);
        assertThat(a2.idempotent()).isTrue();

        SessionStore.Channel ch = server.sessionStore().load(CHANNEL_ID).orElseThrow();
        assertThat(ch.spent()).isEqualTo(100);
        assertThat(ch.units()).isEqualTo(1);
        // Replay did not advance spent; only explicit deduct calls bill.
    }

    @Test
    @DisplayName("Channel exhaustion at exact accepted bound — last byte deductible")
    void exhaustion_at_exact_bound() {
        seedChannel(BigInteger.valueOf(1_000));

        BigInteger cum = BigInteger.valueOf(500);
        server.acceptVoucher(CHANNEL_ID, cum, signVoucher(cum));

        // Spend the entire 500 in one shot
        SessionStore.DeductResult full = server.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(500));
        assertThat(full.spent()).isEqualTo(500);

        // Next 1-unit call should fail
        assertThatThrownBy(() -> server.deductFromChannel(CHANNEL_ID, BigInteger.ONE))
            .isInstanceOf(InsufficientBalanceError.class);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private void seedChannel(BigInteger deposit) {
        Receipt.SessionReceipt openResp = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, deposit.toString(), domain.chainId(), "0xopentx");
        when(sa.sessionOpen(any())).thenReturn(openResp);
        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            payerAddr, payeeAddr,
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");
        server.sessionHandler().handleOpen("openBody", init);
    }

    private byte[] signVoucher(BigInteger cum) {
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.voucherStructHash(CHANNEL_ID, cum));
        return Eip712Signer.sign(digest, payerKey);
    }
}
