package com.okx.payments.mpp.seller;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.nonce.NonceProvider;
import com.okx.payments.mpp.nonce.UuidNonceProvider;
import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import com.okx.payments.mpp.sa.HttpSaApiClient;
import com.okx.payments.mpp.sa.SaApiClient;
import com.okx.payments.mpp.sa.SaApiConfig;
import com.okx.payments.mpp.server.EvmChargeMethod;
import com.okx.payments.mpp.server.EvmSessionMethod;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.math.BigInteger;
import java.time.Clock;
import java.time.Duration;
import java.util.Objects;

/**
 * Facade over the Seller SDK — single entry point for the application.
 *
 * <p>Responsibilities:
 * <ul>
 *   <li>{@link #request} — issue a signed {@link Challenge} for the WWW-Authenticate header</li>
 *   <li>{@link #acceptVoucher} — receive an off-band voucher and update the session ledger</li>
 *   <li>{@link #settle} / {@link #close} — merchant-initiated relayer-mode operations</li>
 *   <li>{@link #status} — read-through to SA</li>
 * </ul>
 *
 * <p>{@code verify(Credential)} routing is exposed via {@link #chargeHandler()} /
 * {@link #sessionHandler()} for the {@code MppAdapter} layer (M9). Direct callers can use those
 * sub-handlers directly when not behind {@code PaymentRouter}.
 */
public final class MppServer {

    private static final Logger log = LoggerFactory.getLogger(MppServer.class);

    private final ChallengeBuilder challengeBuilder;
    private final ChargeHandler chargeHandler;
    private final SessionHandler sessionHandler;
    private final EvmChargeMethod evmChargeMethod;
    private final EvmSessionMethod evmSessionMethod;
    private final SessionStore sessionStore;
    private final EvmPaymentChannelDomain domain;
    private final SaApiClient sa;
    private final PayeeAuthSigner payeeSigner;

    private MppServer(Builder b) {
        this.domain = b.domain != null ? b.domain : EvmPaymentChannelDomain.fromEnv();
        this.sa = b.saApiClient != null ? b.saApiClient
            : (b.saApiConfig != null ? new HttpSaApiClient(b.saApiConfig) : null);
        if (this.sa == null) {
            throw new IllegalStateException("either saApiClient or saApiConfig must be supplied");
        }

        this.payeeSigner = Objects.requireNonNull(b.payeeAuthSigner,
            "payeeAuthSigner is required (use PrivateKeyPayeeAuthSigner.fromEnvVar/fromHex/fromBytes)");

        ChallengeSigner cs = new ChallengeSigner(Objects.requireNonNull(b.challengeSecretKey,
            "challengeSecretKey is required"));
        this.challengeBuilder = new ChallengeBuilder(
            cs, new Base64UrlJson(new ObjectMapper()),
            b.clock != null ? b.clock : Clock.systemUTC(),
            b.challengeTtl != null ? b.challengeTtl : Duration.ofMinutes(5));

        this.sessionStore = b.sessionStore != null ? b.sessionStore : new InMemorySessionStore();

        BigInteger deadline = b.defaultDeadline != null ? b.defaultDeadline
            : BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);   // uint256 max
        BigInteger minDelta = b.minVoucherDelta != null ? b.minVoucherDelta : BigInteger.ZERO;
        NonceProvider nonceProvider = b.nonceProvider != null ? b.nonceProvider : new UuidNonceProvider();

        this.chargeHandler = new ChargeHandler(this.sa);
        this.sessionHandler = new SessionHandler(
            this.sa, this.sessionStore, this.payeeSigner, nonceProvider,
            this.domain, deadline, minDelta,
            b.clock != null ? b.clock : Clock.systemUTC());
        // T1-5: wire ChallengeBuilder into façades so they HMAC-verify and time-check the echoed
        // challenge before dispatch. Direct façade callers (custom adapters, tests bypassing
        // MppPaymentInterceptor) inherit the same gating that the interceptor already enforces
        // — SA does NOT enforce challenge.expires on session paths, so this is the only line
        // of defence against replay for those callers.
        this.evmChargeMethod = new EvmChargeMethod(this.chargeHandler, this.challengeBuilder);
        this.evmSessionMethod = new EvmSessionMethod(this.sessionHandler, this.challengeBuilder);

        if (this.sessionStore instanceof InMemorySessionStore) {
            log.warn("MppServer using InMemorySessionStore — server restart will lose mid-session "
                + "vouchers (only on-chain settled amounts persist). Plug in a durable SessionStore "
                + "for production deployments.");
        }
    }

    public static Builder builder() {
        return new Builder();
    }

    // ── Public API ────────────────────────────────────────────────────────────

    public Challenge request(String realm, Intent intent, Object requestPayload) {
        return challengeBuilder.build(realm, Method.EVM, intent, requestPayload);
    }

    public ChallengeBuilder challengeBuilder() {
        return challengeBuilder;
    }

    public ChargeHandler chargeHandler() {
        return chargeHandler;
    }

    public SessionHandler sessionHandler() {
        return sessionHandler;
    }

    /** High-level auto-dispatching session entry point — preferred over {@link #sessionHandler()}. */
    public EvmSessionMethod evmSessionMethod() {
        return evmSessionMethod;
    }

    /** High-level auto-dispatching charge entry point — preferred over {@link #chargeHandler()}. */
    public EvmChargeMethod evmChargeMethod() {
        return evmChargeMethod;
    }

    public SessionStore sessionStore() {
        return sessionStore;
    }

    public EvmPaymentChannelDomain domain() {
        return domain;
    }

    public SaApiClient saApiClient() {
        return sa;
    }

    public PayeeAuthSigner payeeAuthSigner() {
        return payeeSigner;
    }

    public SessionHandler.VoucherAck acceptVoucher(String channelId, BigInteger cum, byte[] sig) {
        return sessionHandler.acceptVoucher(channelId, cum, sig);
    }

    /**
     * Charge {@code amount} against a session channel — Rust mpp parity (see
     * {@link SessionHandler#deductFromChannel}). Throws {@code InsufficientBalanceError}
     * (70015) when the channel's accepted-but-unspent balance is insufficient.
     */
    public SessionStore.DeductResult deductFromChannel(String channelId, BigInteger amount) {
        return sessionHandler.deductFromChannel(channelId, amount);
    }

    public Receipt.SessionReceipt settle(String channelId) {
        return sessionHandler.settle(channelId, null);
    }

    public Receipt.SessionReceipt settle(String channelId, BigInteger overrideCum) {
        return sessionHandler.settle(channelId, overrideCum);
    }

    public Receipt.SessionReceipt close(String channelId) {
        return sessionHandler.close(channelId, null);
    }

    public Receipt.SessionReceipt close(String channelId, BigInteger finalCum) {
        return sessionHandler.close(channelId, finalCum);
    }

    public SessionStatusResponse status(String channelId) {
        return sessionHandler.status(channelId);
    }

    // ── Builder ───────────────────────────────────────────────────────────────

    public static final class Builder {
        private SaApiConfig saApiConfig;
        private SaApiClient saApiClient;
        private byte[] challengeSecretKey;
        private PayeeAuthSigner payeeAuthSigner;
        private EvmPaymentChannelDomain domain;
        private SessionStore sessionStore;
        private NonceProvider nonceProvider;
        private BigInteger defaultDeadline;
        private BigInteger minVoucherDelta;
        private Duration challengeTtl;
        private Clock clock;

        public Builder saApiConfig(SaApiConfig v) { this.saApiConfig = v; return this; }
        public Builder saApiClient(SaApiClient v) { this.saApiClient = v; return this; }
        public Builder challengeSecretKey(byte[] v) { this.challengeSecretKey = v; return this; }
        public Builder payeeAuthSigner(PayeeAuthSigner v) { this.payeeAuthSigner = v; return this; }
        public Builder domain(EvmPaymentChannelDomain v) { this.domain = v; return this; }
        public Builder sessionStore(SessionStore v) { this.sessionStore = v; return this; }
        public Builder nonceProvider(NonceProvider v) { this.nonceProvider = v; return this; }
        public Builder deadlineDefault(BigInteger v) { this.defaultDeadline = v; return this; }
        public Builder minVoucherDelta(BigInteger v) { this.minVoucherDelta = v; return this; }
        public Builder challengeTtl(Duration v) { this.challengeTtl = v; return this; }
        public Builder clock(Clock v) { this.clock = v; return this; }
        public MppServer build() { return new MppServer(this); }
    }
}
