package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.AmountExceedsDepositError;
import com.okx.payments.mpp.errors.BadRequestError;
import com.okx.payments.mpp.errors.ChannelClosedError;
import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.InsufficientBalanceError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.errors.InvalidSignatureError;
import com.okx.payments.mpp.errors.VoucherDeltaTooSmallError;
import com.okx.payments.mpp.nonce.NonceProvider;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.math.BigInteger;
import java.time.Clock;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.never;
import static org.mockito.Mockito.times;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

class SessionHandlerTest {

    private static final String CHANNEL_ID =
        "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f";

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();
    private final BigInteger uint256Max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);

    /** Payer signs vouchers (channel.authorizedSigner = 0x0). Use deterministic key for tests. */
    private final ECKeyPair payerKey = ECKeyPair.create(BigInteger.valueOf(7));
    private final String payerAddr = "0x" + Keys.getAddress(payerKey);

    /** Payee signs SettleAuth/CloseAuth. */
    private final ECKeyPair payeeKey = ECKeyPair.create(BigInteger.valueOf(13));
    private final String payeeAddr = "0x" + Keys.getAddress(payeeKey);

    private SaApiClient sa;
    private SessionStore store;
    private SessionHandler handler;

    @BeforeEach
    void setup() {
        sa = mock(SaApiClient.class);
        store = new InMemorySessionStore();

        PayeeAuthSigner payeeSigner = PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16));
        NonceProvider nonceProv = (p, c) -> BigInteger.valueOf(42);

        handler = new SessionHandler(sa, store, payeeSigner, nonceProv,
            domain, uint256Max, BigInteger.ZERO, Clock.systemUTC());

        // Pre-load a channel with deposit=1_000_000 so tests don't need to call handleOpen first.
        store.put(new SessionStore.Channel(
            CHANNEL_ID, payerAddr,
            payeeAddr,
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(1_000_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));
    }

    // ── acceptVoucher: D-NEW §submit_voucher 9-step ───────────────────────────

    @Test
    void accept_voucher_first_time_succeeds_and_persists_signature() {
        BigInteger cum = BigInteger.valueOf(100);
        byte[] sig = signVoucher(cum);

        SessionHandler.VoucherAck ack = handler.acceptVoucher(CHANNEL_ID, cum, sig);
        assertThat(ack.acceptedCumulativeAmount()).isEqualTo(cum);
        assertThat(ack.idempotent()).isFalse();

        SessionStore.Channel ch = store.load(CHANNEL_ID).orElseThrow();
        assertThat(ch.lastAccepted()).isEqualTo(cum);
        assertThat(ch.lastVoucherSignature()).isEqualTo(sig);
    }

    @Test
    void accept_voucher_idempotent_replay_with_same_cum_and_sig() {
        BigInteger cum = BigInteger.valueOf(100);
        byte[] sig = signVoucher(cum);
        handler.acceptVoucher(CHANNEL_ID, cum, sig);

        SessionHandler.VoucherAck ack2 = handler.acceptVoucher(CHANNEL_ID, cum, sig);
        assertThat(ack2.idempotent()).isTrue();
        assertThat(ack2.acceptedCumulativeAmount()).isEqualTo(cum);
    }

    @Test
    void accept_voucher_same_cum_different_sig_rejected() {
        BigInteger cum = BigInteger.valueOf(100);
        byte[] sig1 = signVoucher(cum);
        handler.acceptVoucher(CHANNEL_ID, cum, sig1);

        // Forge a "different" sig with bad signer (use payee key instead of payer)
        byte[] digest = voucherDigest(cum);
        byte[] sigByOtherKey = Eip712Signer.sign(digest, payeeKey);

        assertThatThrownBy(() -> handler.acceptVoucher(CHANNEL_ID, cum, sigByOtherKey))
            .isInstanceOf(InvalidSignatureError.class)
            .hasMessageContaining("non-matching signature");
    }

    @Test
    void accept_voucher_strict_monotonicity_violation() {
        handler.acceptVoucher(CHANNEL_ID, BigInteger.valueOf(200), signVoucher(BigInteger.valueOf(200)));
        assertThatThrownBy(() -> handler.acceptVoucher(CHANNEL_ID,
            BigInteger.valueOf(100), signVoucher(BigInteger.valueOf(100))))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("monotonicity violated");
    }

    @Test
    void accept_voucher_exceeds_deposit_rejected() {
        BigInteger over = BigInteger.valueOf(1_000_001);
        assertThatThrownBy(() -> handler.acceptVoucher(CHANNEL_ID, over, signVoucher(over)))
            .isInstanceOf(AmountExceedsDepositError.class);
    }

    @Test
    void accept_voucher_below_min_delta_rejected() {
        // Re-create handler with non-zero min delta = 50
        PayeeAuthSigner payeeSigner = PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16));
        NonceProvider nonceProv = (p, c) -> BigInteger.valueOf(42);
        handler = new SessionHandler(sa, store, payeeSigner, nonceProv,
            domain, uint256Max, BigInteger.valueOf(50), Clock.systemUTC());

        handler.acceptVoucher(CHANNEL_ID, BigInteger.valueOf(100), signVoucher(BigInteger.valueOf(100)));
        assertThatThrownBy(() -> handler.acceptVoucher(CHANNEL_ID,
            BigInteger.valueOf(149), signVoucher(BigInteger.valueOf(149))))
            .isInstanceOf(VoucherDeltaTooSmallError.class);

        // Exactly at the threshold passes
        handler.acceptVoucher(CHANNEL_ID, BigInteger.valueOf(150), signVoucher(BigInteger.valueOf(150)));
    }

    @Test
    void accept_voucher_invalid_signature_rejected() {
        BigInteger cum = BigInteger.valueOf(100);
        byte[] badSig = new byte[65];   // all zeros — recover will fail
        assertThatThrownBy(() -> handler.acceptVoucher(CHANNEL_ID, cum, badSig))
            .isInstanceOf(InvalidSignatureError.class);
    }

    @Test
    void accept_voucher_unknown_channel_throws_not_found() {
        BigInteger cum = BigInteger.valueOf(100);
        assertThatThrownBy(() -> handler.acceptVoucher("0x" + "ee".repeat(32), cum, signVoucher(cum)))
            .isInstanceOf(ChannelNotFoundError.class);
    }

    // ── settle ───────────────────────────────────────────────────────────────

    @Test
    void settle_short_circuits_when_cum_le_settled_on_chain() {
        // Mark settledOnChain = 500
        store.updateSettledOnChain(CHANNEL_ID, BigInteger.valueOf(500));
        // No accepted voucher yet (lastAccepted=0); settle with override 400 → short-circuit
        Receipt.SessionReceipt r = handler.settle(CHANNEL_ID, BigInteger.valueOf(400));
        assertThat(r.reference()).isNull();
        assertThat(r.deposit()).isEqualTo("1000000");
        verify(sa, never()).sessionSettle(any());
    }

    @Test
    void settle_with_no_voucher_throws_bad_request() {
        // settledOnChain=0 default; cum > 0 implied; lastVoucherSignature is null → settle should refuse
        // We bypass short-circuit by setting overrideCum
        assertThatThrownBy(() -> handler.settle(CHANNEL_ID, BigInteger.valueOf(100)))
            .isInstanceOf(BadRequestError.class)
            .hasMessageContaining("no voucher accepted");
    }

    @Test
    void settle_happy_path_signs_and_calls_sa() {
        BigInteger cum = BigInteger.valueOf(250000);
        handler.acceptVoucher(CHANNEL_ID, cum, signVoucher(cum));

        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "2026-04-01T12:09:30Z",
            CHANNEL_ID, "1000000", 196L, "0xsettletx");
        when(sa.sessionSettle(any())).thenReturn(mock);

        Receipt.SessionReceipt r = handler.settle(CHANNEL_ID, null);
        assertThat(r.reference()).isEqualTo("0xsettletx");
        SessionStore.Channel ch = store.load(CHANNEL_ID).orElseThrow();
        assertThat(ch.settledOnChain()).isEqualTo(cum);
    }

    // ── close ────────────────────────────────────────────────────────────────

    @Test
    void close_waiver_path_when_cum_le_settled_uses_empty_voucher_sig() {
        store.updateSettledOnChain(CHANNEL_ID, BigInteger.valueOf(500));
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "2026-04-01T12:09:30Z",
            CHANNEL_ID, "1000000", 196L, "0xclosetx");
        when(sa.sessionClose(any())).thenReturn(mock);

        Receipt.SessionReceipt r = handler.close(CHANNEL_ID, BigInteger.valueOf(300));
        assertThat(r.reference()).isEqualTo("0xclosetx");
        SessionStore.Channel ch = store.load(CHANNEL_ID).orElseThrow();
        assertThat(ch.status()).isEqualTo(SessionStore.ChannelStatus.CLOSING);
    }

    @Test
    void close_normal_path_requires_voucher() {
        // No voucher accepted yet — close with cum > settledOnChain must throw.
        assertThatThrownBy(() -> handler.close(CHANNEL_ID, BigInteger.valueOf(100)))
            .isInstanceOf(BadRequestError.class)
            .hasMessageContaining("voucherSignature required");
    }

    @Test
    void close_normal_path_after_voucher_accepted() {
        BigInteger cum = BigInteger.valueOf(200);
        handler.acceptVoucher(CHANNEL_ID, cum, signVoucher(cum));

        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "2026-04-01T12:09:30Z",
            CHANNEL_ID, "1000000", 196L, "0xclose");
        when(sa.sessionClose(any())).thenReturn(mock);

        handler.close(CHANNEL_ID, null);
        SessionStore.Channel ch = store.load(CHANNEL_ID).orElseThrow();
        assertThat(ch.status()).isEqualTo(SessionStore.ChannelStatus.CLOSING);
    }

    @Test
    void close_already_closed_throws() {
        store.markStatus(CHANNEL_ID, SessionStore.ChannelStatus.CLOSED);
        assertThatThrownBy(() -> handler.close(CHANNEL_ID, null))
            .isInstanceOf(ChannelClosedError.class);
    }

    // ── status read-through ───────────────────────────────────────────────────

    @Test
    void status_delegates_to_sa() {
        SessionStatusResponse mock = new SessionStatusResponse(
            CHANNEL_ID, payerAddr, payeeAddr, "0xtoken",
            "1000000", "200000", "OPEN", "800000");
        when(sa.sessionStatus(CHANNEL_ID)).thenReturn(mock);
        assertThat(handler.status(CHANNEL_ID).deposit()).isEqualTo("1000000");
    }

    // ── deductFromChannel — multi-URL billing per Rust mpp deduct_from_channel ─

    @Test
    void deduct_advances_spent_under_accepted_voucher_cap() {
        BigInteger cum = BigInteger.valueOf(1000);
        handler.acceptVoucher(CHANNEL_ID, cum, signVoucher(cum));

        SessionStore.DeductResult r1 = handler.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(100));
        assertThat(r1.spent()).isEqualTo(100);
        assertThat(r1.units()).isEqualTo(1);

        SessionStore.DeductResult r2 = handler.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(300));
        assertThat(r2.spent()).isEqualTo(400);
        assertThat(r2.units()).isEqualTo(2);
    }

    @Test
    void deduct_exhausts_then_throws_70015_until_voucher_increments() {
        BigInteger cum1 = BigInteger.valueOf(500);
        handler.acceptVoucher(CHANNEL_ID, cum1, signVoucher(cum1));
        handler.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(450));

        assertThatThrownBy(() ->
            handler.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(100)))
            .isInstanceOf(InsufficientBalanceError.class)
            .hasMessageContaining("requested 100")
            .hasMessageContaining("available 50");

        // After the client submits a higher voucher, deduct succeeds again.
        BigInteger cum2 = BigInteger.valueOf(1000);
        handler.acceptVoucher(CHANNEL_ID, cum2, signVoucher(cum2));
        SessionStore.DeductResult r = handler.deductFromChannel(CHANNEL_ID, BigInteger.valueOf(100));
        assertThat(r.spent()).isEqualTo(550);
    }

    @Test
    void deduct_zero_amount_rejected() {
        assertThatThrownBy(() ->
            handler.deductFromChannel(CHANNEL_ID, BigInteger.ZERO))
            .isInstanceOf(BadRequestError.class)
            .hasMessageContaining("amount must be > 0");
    }

    @Test
    void deduct_on_closing_channel_rejected() {
        store.markStatus(CHANNEL_ID, SessionStore.ChannelStatus.CLOSING);
        assertThatThrownBy(() ->
            handler.deductFromChannel(CHANNEL_ID, BigInteger.ONE))
            .isInstanceOf(com.okx.payments.mpp.errors.ChannelClosingError.class);
    }

    @Test
    void deduct_unknown_channel_throws_not_found() {
        assertThatThrownBy(() ->
            handler.deductFromChannel("0x" + "ff".repeat(32), BigInteger.ONE))
            .isInstanceOf(ChannelNotFoundError.class);
    }

    // ── Phase 3 — strict open requires SA-supplied deposit ────────────────────

    @Test
    void open_throws_when_sa_response_omits_deposit() {
        // T1-4 requires payee == payeeSigner.address(); use payeeAddr to pass the cross-check
        // and reach the deposit-null branch.
        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            "0xpayer000000000000000000000000000000000aa",
            payeeAddr,
            "0xtoken000000000000000000000000000000000cc",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            "0x" + "ab".repeat(32), null, 196L, "0xopentx");   // ← deposit null
        when(sa.sessionOpen(any())).thenReturn(mock);

        assertThatThrownBy(() -> handler.handleOpen("body", init))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("missing required deposit");
    }

    @Test
    void topup_throws_when_sa_response_omits_deposit() {
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, null, 196L, "0xtopup");                 // ← deposit null
        when(sa.sessionTopUp(any())).thenReturn(mock);

        assertThatThrownBy(() -> handler.handleTopUp("body"))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("missing required deposit");
    }

    @Test
    void topup_updates_local_deposit_from_sa() {
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "1500000", 196L, "0xtopup");
        when(sa.sessionTopUp(any())).thenReturn(mock);

        handler.handleTopUp("body");
        SessionStore.Channel c = store.load(CHANNEL_ID).orElseThrow();
        assertThat(c.deposit()).isEqualTo(BigInteger.valueOf(1_500_000));
    }

    @Test
    void open_persists_channel_state() {
        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            "0xpayer000000000000000000000000000000000aa",
            payeeAddr,            // T1-4: must match payeeSigner.address()
            "0xtoken000000000000000000000000000000000cc",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            "0x" + "ab".repeat(32), "5000", 196L, "0xopentx");
        when(sa.sessionOpen(any())).thenReturn(mock);

        Receipt.SessionReceipt r = handler.handleOpen("body", init);
        assertThat(r.channelId()).isEqualTo("0x" + "ab".repeat(32));
        SessionStore.Channel ch = store.load(r.channelId()).orElseThrow();
        assertThat(ch.deposit()).isEqualTo(new BigInteger("5000"));
    }

    // ── T1-4: signer/payee cross-check at open ────────────────────────────────

    @Test
    void open_rejects_when_signer_address_mismatches_payee() {
        // ChannelInit.payee deliberately not equal to payeeSigner.address(): T1-4 must catch this
        // BEFORE calling SA. If the guard fails, the test will succeed (false positive) only if
        // SA mock returns happily — assert SA was never invoked.
        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            "0xpayer000000000000000000000000000000000aa",
            "0xpayee000000000000000000000000000000000bb",            // mismatched
            "0xtoken000000000000000000000000000000000cc",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");

        assertThatThrownBy(() -> handler.handleOpen("body", init))
            .isInstanceOf(BadRequestError.class)
            .hasMessageContaining("does not match challenge.request.recipient");
        verify(sa, never()).sessionOpen(any());
    }

    @Test
    void open_accepts_when_payee_matches_signer_case_insensitive() {
        // Same payee in upper-case still matches the lowercase signer address.
        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            "0xpayer000000000000000000000000000000000aa",
            payeeAddr.toUpperCase().replace("0X", "0x"),
            "0xtoken000000000000000000000000000000000cc",
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000");
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            "0x" + "cd".repeat(32), "1000", 196L, "0xtx");
        when(sa.sessionOpen(any())).thenReturn(mock);

        Receipt.SessionReceipt r = handler.handleOpen("body", init);
        assertThat(r.channelId()).isEqualTo("0x" + "cd".repeat(32));
    }

    // ── T1-2: settle/close locks ──────────────────────────────────────────────

    @Test
    void close_evicts_per_channel_lock() throws Exception {
        // Pre-condition: accept a voucher so close has something to verify.
        BigInteger cum = BigInteger.valueOf(200);
        handler.acceptVoucher(CHANNEL_ID, cum, signVoucher(cum));
        // After acceptVoucher, channelLocks contains an entry for CHANNEL_ID.
        assertThat(channelLockExists(CHANNEL_ID)).isTrue();

        when(sa.sessionClose(any())).thenReturn(new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "1000000", 196L, "0xclosetx"));

        handler.close(CHANNEL_ID, null);

        // T1-2: lock entry removed after successful close to bound memory growth.
        assertThat(channelLockExists(CHANNEL_ID)).isFalse();
    }

    @Test
    void settle_takes_per_channel_lock() throws Exception {
        // The simplest observable proof of the lock is that the lock entry is created on the
        // first settle invocation (even though acceptVoucher would already create it). Force a
        // settle path that doesn't go through acceptVoucher: use the seed channel and override
        // cum equal to settled to short-circuit (no SA call needed).
        // Pre-condition: bring the channel to settled = 100 so we can short-circuit.
        store.updateSettledOnChain(CHANNEL_ID, BigInteger.valueOf(100));

        Receipt.SessionReceipt r = handler.settle(CHANNEL_ID, BigInteger.valueOf(50));

        assertThat(r.status()).isEqualTo("success");
        // Lock created during settle (even though body short-circuited before SA).
        assertThat(channelLockExists(CHANNEL_ID)).isTrue();
        verify(sa, never()).sessionSettle(any());
    }

    @SuppressWarnings("unchecked")
    private boolean channelLockExists(String channelId) throws Exception {
        java.lang.reflect.Field f = SessionHandler.class.getDeclaredField("channelLocks");
        f.setAccessible(true);
        java.util.Map<String, ?> locks = (java.util.Map<String, ?>) f.get(handler);
        return locks.containsKey(channelId);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private byte[] voucherDigest(BigInteger cum) {
        return Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.voucherStructHash(CHANNEL_ID, cum));
    }

    private byte[] signVoucher(BigInteger cum) {
        return Eip712Signer.sign(voucherDigest(cum), payerKey);
    }
}
