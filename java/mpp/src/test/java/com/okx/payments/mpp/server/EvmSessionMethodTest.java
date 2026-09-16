package com.okx.payments.mpp.server;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.errors.InvalidChallengeError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Challenge;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Intent;
import com.okx.payments.mpp.protocol.Method;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.encoding.Base64UrlJson;
import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import com.okx.payments.mpp.seller.ChallengeBuilder;
import com.okx.payments.mpp.seller.ChallengeSigner;
import com.okx.payments.mpp.seller.InMemorySessionStore;
import com.okx.payments.mpp.seller.PrivateKeyPayeeAuthSigner;
import com.okx.payments.mpp.seller.SessionHandler;
import com.okx.payments.mpp.seller.SessionStore;
import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import com.okx.payments.mpp.sa.SaApiClient;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.ArgumentCaptor;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.math.BigInteger;
import java.time.Clock;
import java.time.Duration;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.mock;
import static org.mockito.Mockito.verify;
import static org.mockito.Mockito.when;

/** Auto-dispatch correctness for EvmSessionMethod (Rust mpp parity at SessionMethod.verify_session). */
class EvmSessionMethodTest {

    private static final String CHANNEL_ID = "0x" + "ab".repeat(32);
    private static final String TOKEN = "0x74b7F1633b89720027F6196A17a631aC6dE26d22";

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();
    private final BigInteger uint256Max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);
    private final ObjectMapper mapper = new ObjectMapper();

    private final ECKeyPair payerKey = ECKeyPair.create(BigInteger.valueOf(7));
    private final String payerAddr = "0x" + Keys.getAddress(payerKey);
    private final String payeeAddr = "0x" + Keys.getAddress(ECKeyPair.create(BigInteger.valueOf(13)));

    private SaApiClient sa;
    private SessionStore store;
    private EvmSessionMethod method;

    @BeforeEach
    void setup() {
        sa = mock(SaApiClient.class);
        store = new InMemorySessionStore();
        SessionHandler handler = new SessionHandler(sa, store,
            PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16)),
            (p, c) -> BigInteger.valueOf(42),
            domain, uint256Max, BigInteger.ZERO, Clock.systemUTC());
        method = new EvmSessionMethod(handler);
    }

    private SessionRequest sessionRequest(String amount) {
        return new SessionRequest(amount, TOKEN, payeeAddr, "request", null, null, null,
            new SessionMethodDetails(domain.chainId(), domain.escrowAddress(), null, "0", null));
    }

    @Test
    void open_routes_to_handleOpen_and_persists_channel() throws Exception {
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "5000", domain.chainId(), "0xopentx");
        when(sa.sessionOpen(any())).thenReturn(mock);

        String body = "{\"action\":\"open\",\"type\":\"transaction\",\"channelId\":\"" + CHANNEL_ID
            + "\",\"salt\":\"0x" + "00".repeat(32) + "\","
            + "\"authorization\":{\"type\":\"eip-3009\",\"from\":\"" + payerAddr
            + "\",\"to\":\"" + domain.escrowAddress() + "\",\"value\":\"5000\","
            + "\"validAfter\":\"0\",\"validBefore\":\"99999999\","
            + "\"nonce\":\"0x" + "11".repeat(32) + "\","
            + "\"signature\":\"0x" + "22".repeat(65) + "\"},"
            + "\"cumulativeAmount\":\"0\",\"voucherSignature\":\"0x" + "33".repeat(65) + "\"}";
        Credential credential = new Credential(null, mapper.readTree(body), null);

        SessionResult r = method.verifySession(credential, sessionRequest("100"));

        assertThat(r).isInstanceOf(SessionResult.OpenResult.class);
        assertThat(r.action()).isEqualTo("open");
        assertThat(r.channelId()).isEqualTo(CHANNEL_ID);
        assertThat(store.load(CHANNEL_ID)).isPresent();
        assertThat(store.load(CHANNEL_ID).get().deposit()).isEqualTo(5000);
    }

    @Test
    void voucher_accepts_without_implicit_deduct() throws Exception {
        // T1-6: handleVoucher no longer auto-deducts even when request.amount > 0.
        // Callers must invoke deductFromChannel explicitly.
        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        BigInteger cum = BigInteger.valueOf(500);
        String sigHex = "0x" + bytesToHex(signVoucher(cum));

        String body = "{\"action\":\"voucher\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"" + cum + "\",\"signature\":\"" + sigHex + "\"}";
        Credential credential = new Credential(null, mapper.readTree(body), null);

        SessionResult r = method.verifySession(credential, sessionRequest("100"));

        assertThat(r).isInstanceOf(SessionResult.VoucherResult.class);
        SessionResult.VoucherResult v = (SessionResult.VoucherResult) r;
        assertThat(v.acceptedCumulativeAmount()).isEqualTo(cum);
        assertThat(v.idempotent()).isFalse();
        // Voucher was accepted (highest_voucher updated in store) but spent/units stayed at 0.
        assertThat(v.spent()).isEqualTo(BigInteger.ZERO);
        assertThat(v.units()).isEqualTo(0L);
        assertThat(store.load(CHANNEL_ID).orElseThrow().lastAccepted()).isEqualTo(cum);
        assertThat(store.load(CHANNEL_ID).orElseThrow().spent()).isEqualTo(BigInteger.ZERO);
    }

    @Test
    void voucher_with_zero_request_amount_skips_deduct() throws Exception {
        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        BigInteger cum = BigInteger.valueOf(300);
        String sigHex = "0x" + bytesToHex(signVoucher(cum));
        String body = "{\"action\":\"voucher\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"" + cum + "\",\"signature\":\"" + sigHex + "\"}";
        Credential credential = new Credential(null, mapper.readTree(body), null);

        // request.amount = 0 → caller wants pure voucher acceptance, no implicit billing
        SessionResult r = method.verifySession(credential, sessionRequest("0"));
        SessionResult.VoucherResult v = (SessionResult.VoucherResult) r;
        assertThat(v.spent()).isEqualTo(BigInteger.ZERO);
        assertThat(v.units()).isEqualTo(0L);
    }

    @Test
    void close_routes_to_handler_close() throws Exception {
        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.valueOf(500),
            new byte[65], BigInteger.valueOf(500), SessionStore.ChannelStatus.OPEN));

        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "10000", domain.chainId(), "0xclosetx");
        when(sa.sessionClose(any())).thenReturn(mock);

        String body = "{\"action\":\"close\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"500\",\"signature\":\"0x" + "44".repeat(65) + "\"}";
        Credential credential = new Credential(null, mapper.readTree(body), null);

        SessionResult r = method.verifySession(credential, null);

        assertThat(r).isInstanceOf(SessionResult.CloseResult.class);
        assertThat(r.channelId()).isEqualTo(CHANNEL_ID);
    }

    @Test
    void unknown_action_throws_invalid_payload() throws Exception {
        Credential credential = new Credential(null,
            mapper.readTree("{\"action\":\"settle\",\"channelId\":\"" + CHANNEL_ID + "\"}"), null);
        assertThatThrownBy(() -> method.verifySession(credential, null))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("unknown session action: 'settle'");
    }

    @Test
    void empty_payload_throws_invalid_payload() {
        Credential credential = new Credential(null, null, null);
        assertThatThrownBy(() -> method.verifySession(credential, null))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("payload is missing");
    }

    // ── T1-1: topUp body strips `challenge` field ─────────────────────────────

    @Test
    void topup_body_strips_challenge_and_keeps_source_and_payload() throws Exception {
        // SA MppSessionTopUpReqDTO accepts only {source, payload} — Rust SDK also strips
        // challenge in handle_topup. Capture the body we hand to SA and assert shape.
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "15000", domain.chainId(), "0xtopup");
        when(sa.sessionTopUp(any())).thenReturn(mock);

        // Pre-seed channel so handleTopUp can locate it
        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        Challenge challenge = new Challenge(
            "ch-id-1", "realm", Method.EVM, Intent.SESSION, "eyJhbW91bnQiOiIxIn0",
            "2030-01-01T00:00:00Z", null, null, null);
        String body = "{\"action\":\"topUp\",\"type\":\"transaction\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"authorization\":{\"type\":\"eip-3009\",\"from\":\"" + payerAddr
            + "\",\"to\":\"" + domain.escrowAddress() + "\",\"value\":\"5000\","
            + "\"validAfter\":\"0\",\"validBefore\":\"99999999\","
            + "\"nonce\":\"0x" + "55".repeat(32) + "\"},"
            + "\"signature\":\"0x" + "66".repeat(65) + "\","
            + "\"additionalDeposit\":\"5000\",\"topUpSalt\":\"0x" + "77".repeat(32) + "\"}";
        Credential credential = new Credential(challenge, mapper.readTree(body), null);

        method.verifySession(credential, sessionRequest("0"));

        ArgumentCaptor<Object> bodyCaptor = ArgumentCaptor.forClass(Object.class);
        verify(sa).sessionTopUp(bodyCaptor.capture());
        @SuppressWarnings("unchecked")
        Map<String, Object> sent = (Map<String, Object>) bodyCaptor.getValue();
        assertThat(sent.keySet()).containsExactlyInAnyOrder("payload");      // no challenge, no source
        assertThat(sent).doesNotContainKey("challenge");
    }

    @Test
    void topup_body_includes_source_when_present() throws Exception {
        // For hash-mode topUp the SDK MUST forward source — verify it lands in the body.
        Receipt.SessionReceipt mock = new Receipt.SessionReceipt(
            Method.EVM, Intent.SESSION, "success", "ts",
            CHANNEL_ID, "15000", domain.chainId(), "0xtopup-hash");
        when(sa.sessionTopUp(any())).thenReturn(mock);

        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        String did = "did:pkh:eip155:" + domain.chainId() + ":" + payerAddr;
        String body = "{\"action\":\"topUp\",\"type\":\"hash\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"hash\":\"0x" + "ab".repeat(32) + "\",\"additionalDeposit\":\"5000\"}";
        Credential credential = new Credential(null, mapper.readTree(body), did);

        method.verifySession(credential, sessionRequest("0"));

        ArgumentCaptor<Object> bodyCaptor = ArgumentCaptor.forClass(Object.class);
        verify(sa).sessionTopUp(bodyCaptor.capture());
        @SuppressWarnings("unchecked")
        Map<String, Object> sent = (Map<String, Object>) bodyCaptor.getValue();
        assertThat(sent).doesNotContainKey("challenge");
        assertThat(sent).containsEntry("source", did);
        assertThat(sent).containsKey("payload");
    }

    // ── T1-7: hash-mode topUp validates source DID ────────────────────────────

    @Test
    void topup_hash_mode_missing_source_throws_invalid_payload() throws Exception {
        String body = "{\"action\":\"topUp\",\"type\":\"hash\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"hash\":\"0x" + "ab".repeat(32) + "\",\"additionalDeposit\":\"5000\"}";
        Credential credential = new Credential(null, mapper.readTree(body), null);     // source = null

        assertThatThrownBy(() -> method.verifySession(credential, sessionRequest("0")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("hash mode credential missing source");
    }

    @Test
    void topup_hash_mode_wrong_chainid_in_source_throws_invalid_payload() throws Exception {
        String wrongDid = "did:pkh:eip155:999:" + payerAddr;           // wrong chainId
        String body = "{\"action\":\"topUp\",\"type\":\"hash\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"hash\":\"0x" + "ab".repeat(32) + "\",\"additionalDeposit\":\"5000\"}";
        Credential credential = new Credential(null, mapper.readTree(body), wrongDid);

        assertThatThrownBy(() -> method.verifySession(credential, sessionRequest("0")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("chainId 999 != expected");
    }

    // ── T1-5: façade gates HMAC + expires when ChallengeBuilder is wired ──────

    @Test
    void facade_rejects_challenge_with_bad_hmac() throws Exception {
        ChallengeBuilder builder = new ChallengeBuilder(
            new ChallengeSigner("test-key".getBytes()),
            new Base64UrlJson(), Clock.systemUTC(), Duration.ofMinutes(5));
        SessionHandler handler = new SessionHandler(sa, store,
            PrivateKeyPayeeAuthSigner.fromHex(BigInteger.valueOf(13).toString(16)),
            (p, c) -> BigInteger.valueOf(42),
            domain, uint256Max, BigInteger.ZERO, Clock.systemUTC());
        EvmSessionMethod gated = new EvmSessionMethod(handler, builder);

        // Forged challenge id — HMAC fails
        Challenge bogus = new Challenge(
            "FORGED-ID", "realm", Method.EVM, Intent.SESSION, "eyJhbW91bnQiOiIxIn0",
            "2030-01-01T00:00:00Z", null, null, null);
        String body = "{\"action\":\"voucher\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"100\",\"signature\":\"0x" + "00".repeat(65) + "\"}";
        Credential credential = new Credential(bogus, mapper.readTree(body), null);

        assertThatThrownBy(() -> gated.verifySession(credential, sessionRequest("0")))
            .isInstanceOf(InvalidChallengeError.class)
            .hasMessageContaining("HMAC verification or has expired");
    }

    @Test
    void facade_without_builder_skips_challenge_check() throws Exception {
        // Interceptor-mode (challengeBuilder = null) — verifySession must NOT gate.
        // This is the default for backward compat; the test re-asserts existing behavior.
        store.put(new SessionStore.Channel(CHANNEL_ID, payerAddr, payeeAddr, TOKEN,
            domain.escrowAddress(), domain.chainId(),
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(10_000), BigInteger.ZERO, null,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN));

        Challenge bogus = new Challenge(
            "FORGED-ID", "realm", Method.EVM, Intent.SESSION, "eyJhbW91bnQiOiIxIn0",
            "2030-01-01T00:00:00Z", null, null, null);
        BigInteger cum = BigInteger.valueOf(500);
        String sigHex = "0x" + bytesToHex(signVoucher(cum));
        String body = "{\"action\":\"voucher\",\"channelId\":\"" + CHANNEL_ID + "\","
            + "\"cumulativeAmount\":\"" + cum + "\",\"signature\":\"" + sigHex + "\"}";
        Credential credential = new Credential(bogus, mapper.readTree(body), null);

        // Default `method` was constructed without ChallengeBuilder — should not throw.
        SessionResult r = method.verifySession(credential, sessionRequest("0"));
        assertThat(r).isInstanceOf(SessionResult.VoucherResult.class);
    }

    @Test
    void open_missing_authorization_from_throws_invalid_payload() throws Exception {
        Credential credential = new Credential(null,
            mapper.readTree("{\"action\":\"open\",\"channelId\":\"" + CHANNEL_ID
                + "\",\"authorization\":{\"type\":\"eip-3009\"}}"), null);
        assertThatThrownBy(() -> method.verifySession(credential, sessionRequest("100")))
            .isInstanceOf(InvalidPayloadError.class)
            .hasMessageContaining("open.authorization.from");
    }

    // ── helpers ────────────────────────────────────────────────────────────────

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
