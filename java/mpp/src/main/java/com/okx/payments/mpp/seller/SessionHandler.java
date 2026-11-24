package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.AmountExceedsDepositError;
import com.okx.payments.mpp.errors.BadRequestError;
import com.okx.payments.mpp.errors.ChannelClosedError;
import com.okx.payments.mpp.errors.ChannelClosingError;
import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.errors.InvalidSignatureError;
import com.okx.payments.mpp.errors.VoucherDeltaTooSmallError;
import com.okx.payments.mpp.nonce.NonceProvider;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.session.payload.SessionClosePayload;
import com.okx.payments.mpp.protocol.session.payload.SessionSettlePayload;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import com.okx.payments.mpp.protocol.encoding.HexUtil;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Verifier;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.math.BigInteger;
import java.security.MessageDigest;
import java.time.Clock;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.locks.ReentrantLock;

/**
 * Session-side handler — drives:
 * <ul>
 *   <li>{@link #handleOpen} / {@link #handleTopUp} — verify(credential) entry from MppAdapter</li>
 *   <li>{@link #acceptVoucher} — out-of-band voucher reception (D-NEW §submit_voucher 9-step)</li>
 *   <li>{@link #settle} / {@link #close} — merchant-initiated; signs SettleAuth/CloseAuth via PayeeAuthSigner</li>
 *   <li>{@link #status} — read-through</li>
 * </ul>
 */
public final class SessionHandler {

    private static final Logger log = LoggerFactory.getLogger(SessionHandler.class);

    private final SaApiClient sa;
    private final SessionStore store;
    private final PayeeAuthSigner payeeSigner;
    private final NonceProvider nonceProvider;
    private final EvmPaymentChannelDomain domain;
    private final BigInteger defaultDeadline;
    private final BigInteger minVoucherDelta;
    private final Clock clock;
    private final ConcurrentHashMap<String, ReentrantLock> channelLocks = new ConcurrentHashMap<>();

    public SessionHandler(SaApiClient sa,
                          SessionStore store,
                          PayeeAuthSigner payeeSigner,
                          NonceProvider nonceProvider,
                          EvmPaymentChannelDomain domain,
                          BigInteger defaultDeadline,
                          BigInteger minVoucherDelta,
                          Clock clock) {
        this.sa = Objects.requireNonNull(sa);
        this.store = Objects.requireNonNull(store);
        this.payeeSigner = Objects.requireNonNull(payeeSigner);
        this.nonceProvider = Objects.requireNonNull(nonceProvider);
        this.domain = Objects.requireNonNull(domain);
        this.defaultDeadline = defaultDeadline == null
            ? BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE)        // uint256 max
            : defaultDeadline;
        this.minVoucherDelta = minVoucherDelta == null ? BigInteger.ZERO : minVoucherDelta;
        this.clock = clock == null ? Clock.systemUTC() : clock;
    }

    // ── Open / TopUp (Client-driven; via verify()) ─────────────────────────────

    /**
     * Forward open credential to SA, persist channel state on success.
     *
     * <p>Aligned with Rust {@code session_method::handlers::handle_open}: SA response
     * MUST include the authoritative {@code deposit} per the current wire contract.
     * Missing field → {@link InvalidPayloadError} — do not silently default to zero.
     */
    public Receipt.SessionReceipt handleOpen(Object credentialBody, ChannelInit init) {
        // T1-4: cross-check the merchant signer key against the challenge-derived payee BEFORE
        // round-tripping SA. A mismatch means the merchant configured PayeeAuthSigner with the
        // wrong key — settle/close would fail server-side at MppVoucherVerifyService
        // .verifySettleAuthorization (recovered != channel.payee). Surfacing here gives an
        // attributable error in one round-trip. Rust mpp parity: handle_open contains the same
        // `signer_addr != expected_payee` guard.
        if (!payeeSigner.address().equalsIgnoreCase(init.payee())) {
            throw (BadRequestError) new BadRequestError(
                "merchant signer address " + payeeSigner.address()
                    + " does not match challenge.request.recipient " + init.payee()
                    + " — configure PayeeAuthSigner with the same key as the receiving address")
                .put("signerAddress", payeeSigner.address())
                .put("expectedPayee", init.payee());
        }

        Receipt.SessionReceipt r = sa.sessionOpen(credentialBody);
        if (r.deposit() == null) {
            throw (InvalidPayloadError) new InvalidPayloadError(
                "SA session/open response missing required deposit field")
                .put("channelId", r.channelId());
        }
        BigInteger deposit = new BigInteger(r.deposit());
        store.put(new SessionStore.Channel(
            r.channelId(),
            init.payer(), init.payee(), init.token(),
            init.escrow(), init.chainId(), init.authorizedSigner(),
            deposit, BigInteger.ZERO, null, BigInteger.ZERO,
            SessionStore.ChannelStatus.OPEN));
        return r;
    }

    /**
     * Forward topUp credential, update local deposit from authoritative SA response.
     *
     * <p>Aligned with Rust strict policy: SA response MUST include the post-topUp
     * {@code deposit}. If absent we fail rather than silently extrapolate from the
     * client-supplied delta (which can drift from on-chain truth).
     */
    public Receipt.SessionReceipt handleTopUp(Object credentialBody) {
        Receipt.SessionReceipt r = sa.sessionTopUp(credentialBody);
        if (r.channelId() != null) {
            if (r.deposit() == null) {
                throw (InvalidPayloadError) new InvalidPayloadError(
                    "SA session/topUp response missing required deposit field")
                    .put("channelId", r.channelId());
            }
            store.updateDeposit(r.channelId(), new BigInteger(r.deposit()));
        }
        return r;
    }

    // ── acceptVoucher (out-of-band) — D-NEW §submit_voucher 9-step ────────────

    public VoucherAck acceptVoucher(String channelId, BigInteger cum, byte[] sig) {
        ReentrantLock lock = channelLocks.computeIfAbsent(channelId, k -> new ReentrantLock());
        lock.lock();
        try {
            // B. load
            SessionStore.Channel ch = store.load(channelId)
                .orElseThrow(() -> (ChannelNotFoundError) new ChannelNotFoundError(
                    "channel not found for voucher: " + channelId).put("channelId", channelId));

            // C. cum > deposit?
            if (cum.compareTo(ch.deposit()) > 0) {
                throw (AmountExceedsDepositError) new AmountExceedsDepositError(
                    "cumulativeAmount > deposit: " + cum + " > " + ch.deposit())
                    .put("channelId", channelId);
            }

            // D. byte-exact idempotent / strict monotonicity
            BigInteger lastAccepted = ch.lastAccepted();
            int cmp = cum.compareTo(lastAccepted);
            if (cmp < 0) {
                throw (InvalidPayloadError) new InvalidPayloadError(
                    "voucher monotonicity violated: cum=" + cum + " < lastAccepted=" + lastAccepted)
                    .put("channelId", channelId);
            }
            if (cmp == 0) {
                if (ch.lastVoucherSignature() != null && MessageDigest.isEqual(sig, ch.lastVoucherSignature())) {
                    return new VoucherAck(cum, true);   // idempotent replay
                }
                throw (InvalidSignatureError) new InvalidSignatureError(
                    "duplicate cumulativeAmount with non-matching signature")
                    .put("channelId", channelId);
            }

            // E. min delta
            if (cum.subtract(lastAccepted).compareTo(minVoucherDelta) < 0) {
                throw (VoucherDeltaTooSmallError) new VoucherDeltaTooSmallError(
                    "delta " + cum.subtract(lastAccepted) + " < minVoucherDelta " + minVoucherDelta)
                    .put("channelId", channelId);
            }

            // F. EIP-712 verify
            byte[] digest = Eip712Hashing.digest(
                Eip712Hashing.domainSeparator(domain),
                Eip712Hashing.voucherStructHash(channelId, cum));
            String expectedSigner = (ch.authorizedSigner() == null
                || ch.authorizedSigner().equalsIgnoreCase("0x0000000000000000000000000000000000000000"))
                ? ch.payer() : ch.authorizedSigner();
            try {
                Eip712Verifier.verify(digest, sig, expectedSigner);
            } catch (Eip712Verifier.Eip712VerifyException e) {
                throw (InvalidSignatureError) new InvalidSignatureError(
                    "voucher signature invalid: " + e.getMessage(), e).put("channelId", channelId);
            }

            // G. CAS update (single retry)
            for (int attempt = 0; attempt < 2; attempt++) {
                if (store.casLastAccepted(channelId, lastAccepted, cum, sig)) {
                    return new VoucherAck(cum, false);
                }
                ch = store.load(channelId).orElseThrow();
                lastAccepted = ch.lastAccepted();
                if (cum.compareTo(lastAccepted) <= 0) {
                    return new VoucherAck(lastAccepted, true);   // someone else won the race
                }
            }
            throw (InvalidPayloadError) new InvalidPayloadError("CAS retry exhausted")
                .put("channelId", channelId);

        } finally {
            lock.unlock();
        }
    }

    // ── Per-route billing — Rust mpp deduct_from_channel ──────────────────────

    /**
     * Atomic per-call billing against the channel's accepted-but-unspent balance.
     * Mirrors Rust mpp {@code deduct_from_channel}.
     *
     * <p>This decouples voucher acceptance from billing — one accepted voucher with
     * cumulative amount {@code C} can be consumed by many distinct route calls until
     * {@code spent} approaches {@code C}, at which point the next deduct returns
     * {@link com.okx.payments.mpp.errors.InsufficientBalanceError} (70015) and the
     * client must increment the voucher.
     *
     * @return post-update {@code (spent, units)} snapshot
     * @throws com.okx.payments.mpp.errors.ChannelNotFoundError if channel is unknown
     * @throws com.okx.payments.mpp.errors.InsufficientBalanceError when available &lt; amount
     * @throws BadRequestError when the channel is in CLOSING/CLOSED state
     */
    public SessionStore.DeductResult deductFromChannel(String channelId, BigInteger amount) {
        if (amount == null || amount.signum() <= 0) {
            throw (BadRequestError) new BadRequestError(
                "deduct amount must be > 0").put("channelId", channelId);
        }
        SessionStore.Channel ch = requireOpen(channelId);  // surfaces CLOSING/CLOSED early
        // requireOpen ensures channel exists; defer further atomicity to the store
        Objects.requireNonNull(ch);
        return store.deduct(channelId, amount);
    }

    // ── Settle / Close — merchant-initiated, signed locally ───────────────────

    public Receipt.SessionReceipt settle(String channelId, BigInteger overrideCum) {
        // T1-2: per-channel lock. Rust mpp settle_with_authorization uses
        // self.channel_locks.lock(channel_id) to serialise concurrent settles. Without this,
        // two threads calling settle(channelId) concurrently each allocate a fresh random nonce
        // and emit two distinct SA HTTP calls (SA's idempotentKey collision in T5-1 dedups
        // server-side, but we still pay the round-trip).
        ReentrantLock lock = channelLocks.computeIfAbsent(channelId, k -> new ReentrantLock());
        lock.lock();
        try {
            return settleInternal(channelId, overrideCum);
        } finally {
            lock.unlock();
        }
    }

    private Receipt.SessionReceipt settleInternal(String channelId, BigInteger overrideCum) {
        SessionStore.Channel ch = requireOpen(channelId);
        BigInteger cum = overrideCum != null ? overrideCum : ch.lastAccepted();

        if (cum.compareTo(ch.settledOnChain()) <= 0) {
            log.info("settle short-circuit: cum {} <= settledOnChain {}", cum, ch.settledOnChain());
            return new Receipt.SessionReceipt(
                com.okx.payments.mpp.protocol.Method.EVM,
                com.okx.payments.mpp.protocol.Intent.SESSION,
                "success",
                java.time.format.DateTimeFormatter.ISO_INSTANT.format(java.time.Instant.now(clock)),
                channelId, ch.deposit().toString(), ch.chainId(), null);
        }
        if (ch.lastVoucherSignature() == null || ch.lastVoucherSignature().length == 0) {
            throw (BadRequestError) new BadRequestError(
                "no voucher accepted yet — cannot settle")
                .put("channelId", channelId);
        }

        BigInteger nonce = nonceProvider.allocate(ch.payee(), channelId);
        BigInteger deadline = defaultDeadline;
        byte[] payeeSig = payeeSigner.signSettleAuthorization(domain,
            new PayeeAuthSigner.AuthData(channelId, cum, nonce, deadline));

        Map<String, Object> body = settlePayloadBody(channelId, cum,
            ch.lastVoucherSignature(), payeeSig, nonce, deadline);

        Receipt.SessionReceipt r = sa.sessionSettle(body);
        store.updateSettledOnChain(channelId, cum);
        return r;
    }

    public Receipt.SessionReceipt close(String channelId, BigInteger overrideCum) {
        // T1-2: per-channel lock, plus eviction after success so channelLocks doesn't accumulate
        // entries for closed channels. Rust mpp close_with_authorization holds the same lock.
        ReentrantLock lock = channelLocks.computeIfAbsent(channelId, k -> new ReentrantLock());
        lock.lock();
        try {
            Receipt.SessionReceipt r = closeInternal(channelId, overrideCum);
            channelLocks.remove(channelId);
            return r;
        } finally {
            lock.unlock();
        }
    }

    private Receipt.SessionReceipt closeInternal(String channelId, BigInteger overrideCum) {
        SessionStore.Channel ch = requireOpenOrClosing(channelId);
        BigInteger cum = overrideCum != null ? overrideCum : ch.lastAccepted();
        boolean waiver = cum.compareTo(ch.settledOnChain()) <= 0;

        String voucherSigHex;
        if (waiver) {
            voucherSigHex = "";
        } else {
            byte[] vsig = ch.lastVoucherSignature();
            if (vsig == null || vsig.length == 0) {
                throw (BadRequestError) new BadRequestError(
                    "voucherSignature required when cumulativeAmount > settledOnChain")
                    .put("channelId", channelId);
            }
            // Defensive 712 verify in case store was tampered with mid-flight
            byte[] digest = Eip712Hashing.digest(
                Eip712Hashing.domainSeparator(domain),
                Eip712Hashing.voucherStructHash(channelId, cum));
            String expectedSigner = (ch.authorizedSigner() == null
                || ch.authorizedSigner().equalsIgnoreCase("0x0000000000000000000000000000000000000000"))
                ? ch.payer() : ch.authorizedSigner();
            try {
                Eip712Verifier.verify(digest, vsig, expectedSigner);
            } catch (Eip712Verifier.Eip712VerifyException e) {
                throw (InvalidSignatureError) new InvalidSignatureError(
                    "stored voucher signature invalid: " + e.getMessage(), e)
                    .put("channelId", channelId);
            }
            voucherSigHex = HexUtil.encode(vsig);
        }

        BigInteger nonce = nonceProvider.allocate(ch.payee(), channelId);
        BigInteger deadline = defaultDeadline;
        byte[] payeeSig = payeeSigner.signCloseAuthorization(domain,
            new PayeeAuthSigner.AuthData(channelId, cum, nonce, deadline));

        Map<String, Object> body = closePayloadBody(channelId, cum,
            voucherSigHex, payeeSig, nonce, deadline);

        Receipt.SessionReceipt r = sa.sessionClose(body);
        store.markStatus(channelId, SessionStore.ChannelStatus.CLOSING);
        return r;
    }

    public SessionStatusResponse status(String channelId) {
        return sa.sessionStatus(channelId);
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private SessionStore.Channel requireOpen(String channelId) {
        SessionStore.Channel ch = store.load(channelId).orElseThrow(() ->
            (ChannelNotFoundError) new ChannelNotFoundError("channel not found: " + channelId)
                .put("channelId", channelId));
        switch (ch.status()) {
            case OPEN -> { return ch; }
            case CLOSING -> throw (ChannelClosingError) new ChannelClosingError("channel closing")
                .put("channelId", channelId);
            case CLOSED -> throw (ChannelClosedError) new ChannelClosedError("channel closed")
                .put("channelId", channelId);
        }
        throw new IllegalStateException();
    }

    private SessionStore.Channel requireOpenOrClosing(String channelId) {
        SessionStore.Channel ch = store.load(channelId).orElseThrow(() ->
            (ChannelNotFoundError) new ChannelNotFoundError("channel not found: " + channelId)
                .put("channelId", channelId));
        if (ch.status() == SessionStore.ChannelStatus.CLOSED) {
            throw (ChannelClosedError) new ChannelClosedError("channel closed")
                .put("channelId", channelId);
        }
        return ch;
    }

    private static Map<String, Object> settlePayloadBody(String channelId, BigInteger cum,
                                                         byte[] voucherSig, byte[] payeeSig,
                                                         BigInteger nonce, BigInteger deadline) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("action", SessionSettlePayload.ACTION);
        payload.put("channelId", channelId);
        payload.put("cumulativeAmount", cum.toString());
        payload.put("voucherSignature", HexUtil.encode(voucherSig));
        payload.put("payeeSignature", HexUtil.encode(payeeSig));
        payload.put("nonce", nonce.toString());
        payload.put("deadline", deadline.toString());
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("payload", payload);
        return body;
    }

    private static Map<String, Object> closePayloadBody(String channelId, BigInteger cum,
                                                        String voucherSigHexOrEmpty, byte[] payeeSig,
                                                        BigInteger nonce, BigInteger deadline) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("action", SessionClosePayload.ACTION);
        payload.put("channelId", channelId);
        payload.put("cumulativeAmount", cum.toString());
        payload.put("voucherSignature", voucherSigHexOrEmpty);
        payload.put("payeeSignature", HexUtil.encode(payeeSig));
        payload.put("nonce", nonce.toString());
        payload.put("deadline", deadline.toString());
        Map<String, Object> body = new LinkedHashMap<>();
        body.put("payload", payload);
        return body;
    }

    public record VoucherAck(BigInteger acceptedCumulativeAmount, boolean idempotent) {}

    /** Required state for {@link #handleOpen} — derived from the open credential payload. */
    public record ChannelInit(String payer, String payee, String token,
                              String escrow, long chainId, String authorizedSigner) {
        public ChannelInit {
            Objects.requireNonNull(payer);
            Objects.requireNonNull(payee);
            Objects.requireNonNull(token);
            Objects.requireNonNull(escrow);
        }
    }
}
