package com.okx.payments.mpp.server;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.okx.payments.mpp.errors.InvalidChallengeError;
import com.okx.payments.mpp.errors.InvalidPayloadError;
import com.okx.payments.mpp.protocol.Credential;
import com.okx.payments.mpp.protocol.Receipt;
import com.okx.payments.mpp.protocol.encoding.DidPkhParser;
import com.okx.payments.mpp.protocol.encoding.HexUtil;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import com.okx.payments.mpp.protocol.session.payload.SessionOpenPayload;
import com.okx.payments.mpp.protocol.session.payload.SessionStatusResponse;
import com.okx.payments.mpp.seller.ChallengeBuilder;
import com.okx.payments.mpp.seller.SessionHandler;
import com.okx.payments.mpp.seller.SessionStore;

import java.math.BigInteger;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;

/**
 * High-level façade implementing the {@code SessionMethod} trait shape from D1 §6.2 and
 * Rust {@code mpp::session_method::EvmSessionMethod}.
 *
 * <p>One entry point — {@link #verifySession(Credential, SessionRequest)} — auto-dispatches
 * by {@code payload.action}:
 * <ul>
 *   <li>{@code open}    → {@code SessionHandler.handleOpen}</li>
 *   <li>{@code topUp}   → {@code SessionHandler.handleTopUp}</li>
 *   <li>{@code voucher} → {@code acceptVoucher} + {@code deductFromChannel(amount)}</li>
 *   <li>{@code close}   → {@code SessionHandler.close}</li>
 * </ul>
 *
 * <p>Plus passthrough helpers for {@link #settle}, {@link #status}, and the M10
 * billing primitive {@link #deduct}.
 *
 * <p>This is the recommended public API (replaces direct {@code SessionHandler} use for
 * most integrators). Rust parity: {@code EvmSessionMethod::verify_session(&credential, &request)}.
 */
public final class EvmSessionMethod {

    private final SessionHandler handler;
    private final ObjectMapper mapper;
    /**
     * Optional challenge gating (T1-5). When non-null and {@code credential.challenge() != null},
     * {@link #verifySession} HMAC-verifies and time-checks the echoed challenge before dispatch.
     * Pass {@code null} for interceptor-mode setups where the routing layer already pre-verifies
     * (see {@code MppPaymentInterceptor}); pass an instance for façade-direct callers (custom
     * adapters, tests) so forged or expired challenges can't reach the session pipeline.
     */
    private final ChallengeBuilder challengeBuilder;

    /**
     * Interceptor-mode constructor — caller (e.g. {@code MppPaymentInterceptor}) pre-verifies the
     * challenge before invoking {@link #verifySession}. New direct integrations should prefer
     * {@link #EvmSessionMethod(SessionHandler, ChallengeBuilder)}.
     */
    public EvmSessionMethod(SessionHandler handler) {
        this(handler, (ChallengeBuilder) null, new ObjectMapper());
    }

    /**
     * Façade-mode constructor — {@code verifySession} HMAC-verifies and time-checks
     * {@code credential.challenge} via {@code challengeBuilder.verifyAlive(...)} before dispatch.
     */
    public EvmSessionMethod(SessionHandler handler, ChallengeBuilder challengeBuilder) {
        this(handler, challengeBuilder, new ObjectMapper());
    }

    public EvmSessionMethod(SessionHandler handler, ObjectMapper mapper) {
        this(handler, null, mapper);
    }

    public EvmSessionMethod(SessionHandler handler, ChallengeBuilder challengeBuilder, ObjectMapper mapper) {
        this.handler = Objects.requireNonNull(handler);
        this.challengeBuilder = challengeBuilder;        // optional — null = interceptor mode
        this.mapper = Objects.requireNonNull(mapper);
    }

    /**
     * Auto-dispatch a session credential by {@code payload.action}.
     *
     * @param credential decoded {@code Authorization: Payment} body
     * @param request    decoded {@code Challenge.request} (Base64UrlJson → SessionRequest);
     *                   used for open (channel init fields) and voucher (per-unit deduct amount)
     */
    public SessionResult verifySession(Credential credential, SessionRequest request) {
        Objects.requireNonNull(credential, "credential");
        JsonNode payload = credential.payload();
        if (payload == null || payload.isNull()) {
            throw new InvalidPayloadError("credential.payload is missing");
        }
        // T1-5 façade gating — when a ChallengeBuilder was supplied, HMAC-verify + expires-check
        // the echoed challenge before dispatch. SA's MppSessionService does NOT enforce expires
        // on session paths (only MppChargeService.guardChallengeNotExpired:178 does, and only
        // for charge) — so this is the only line of defence against forged or replayed challenges
        // for callers that don't sit behind MppPaymentInterceptor.
        if (challengeBuilder != null && credential.challenge() != null
                && !challengeBuilder.verifyAlive(credential.challenge())) {
            throw new InvalidChallengeError(
                "challenge failed HMAC verification or has expired");
        }
        String action = payload.path("action").asText("");
        return switch (action) {
            case "open" -> handleOpen(credential, request);
            case "topUp", "topup" -> handleTopUp(credential, request);
            case "voucher" -> handleVoucher(credential, request);
            case "close" -> handleClose(credential);
            default -> throw new InvalidPayloadError(
                "unknown session action: '" + action + "' (expected open|topUp|voucher|close)");
        };
    }

    /** Per-call billing primitive — bills `amount` against the channel's accepted-but-unspent balance. */
    public SessionStore.DeductResult deduct(String channelId, BigInteger amount) {
        return handler.deductFromChannel(channelId, amount);
    }

    /** Mid-session settle — payee signs SettleAuthorization, SA broadcasts on-chain. */
    public Receipt.SessionReceipt settle(String channelId) {
        return handler.settle(channelId, null);
    }

    /** Mid-session settle with explicit cumulative override. */
    public Receipt.SessionReceipt settle(String channelId, BigInteger overrideCum) {
        return handler.settle(channelId, overrideCum);
    }

    /** Read-through to SA {@code /session/status}. */
    public SessionStatusResponse status(String channelId) {
        return handler.status(channelId);
    }

    /** Underlying handler — exposed for advanced callers needing primitive access. */
    public SessionHandler handler() {
        return handler;
    }

    // ── per-action dispatch ────────────────────────────────────────────────────

    private SessionResult handleOpen(Credential credential, SessionRequest request) {
        Objects.requireNonNull(request, "request (decoded Challenge.request) is required for open");
        JsonNode payload = credential.payload();
        String type = payload.path("type").asText(SessionOpenPayload.TYPE_TRANSACTION);

        // Mirrors Rust mpp-rs `extract_payer_and_signer` (PR okx/payments#9):
        // - transaction mode → payer = payload.authorization.from
        // - hash mode        → payer = parse_did_pkh_eip155(credential.source, chainId)
        long expectedChainId = (request.methodDetails() != null && request.methodDetails().chainId() != null)
            ? request.methodDetails().chainId() : 0L;
        String payer;
        if (SessionOpenPayload.TYPE_HASH.equals(type)) {
            payer = DidPkhParser.parseEvmAddress(credential.source(), expectedChainId);
        } else {
            JsonNode auth = payload.path("authorization");
            payer = requireText(auth, "from", "open.authorization.from");
        }
        String payee = requireNonNull(request.recipient(), "request.recipient");
        String token = requireNonNull(request.currency(), "request.currency");
        if (request.methodDetails() == null) {
            throw new InvalidPayloadError("request.methodDetails missing for session/open");
        }
        String escrow = requireNonNull(request.methodDetails().escrowContract(),
            "request.methodDetails.escrowContract");
        long chainId = request.methodDetails().chainId();
        String authorizedSigner = payload.hasNonNull("authorizedSigner")
            ? payload.path("authorizedSigner").asText() : payer;

        SessionHandler.ChannelInit init = new SessionHandler.ChannelInit(
            payer, payee, token, escrow, chainId, authorizedSigner);

        Object body = mapper.convertValue(credential, java.util.Map.class);
        Receipt.SessionReceipt r = handler.handleOpen(body, init);
        return new SessionResult.OpenResult(r);
    }

    private SessionResult handleTopUp(Credential credential, SessionRequest request) {
        JsonNode payload = credential.payload();
        String type = payload.path("type").asText(SessionOpenPayload.TYPE_TRANSACTION);

        // T1-7: hash-mode topUp requires source DID (SA MppSessionService.topUpHashMode:864
        // calls guardSourcePresent). Pre-validate locally so the SDK returns an attributable
        // InvalidPayloadError instead of round-tripping for a 70000.
        if (SessionOpenPayload.TYPE_HASH.equals(type)) {
            long chainId = (request != null && request.methodDetails() != null
                    && request.methodDetails().chainId() != null)
                ? request.methodDetails().chainId() : 0L;
            DidPkhParser.parseEvmAddress(credential.source(), chainId);
        }

        // T1-1: SA MppSessionTopUpReqDTO defines only {source, payload} — the challenge field is
        // dropped on deserialize today (Jackson lenient) but is wasted bandwidth and would 400 if
        // SA tightens. Rust mpp::session_method::handle_topup explicitly builds the same shape.
        Map<String, Object> body = new LinkedHashMap<>();
        if (credential.source() != null) {
            body.put("source", credential.source());
        }
        body.put("payload", credential.payload());

        Receipt.SessionReceipt r = handler.handleTopUp(body);
        return new SessionResult.TopUpResult(r);
    }

    private SessionResult handleVoucher(Credential credential, SessionRequest request) {
        JsonNode payload = credential.payload();
        String channelId = requireText(payload, "channelId", "voucher.channelId");
        BigInteger cum = parseBigInt(payload, "cumulativeAmount", "voucher.cumulativeAmount");
        byte[] sig = HexUtil.decode(requireText(payload, "signature", "voucher.signature"));

        // T1-6: voucher acceptance is independent of billing — mirrors Rust mpp
        // session_method::handle_voucher which only validates and stores. Callers invoke
        // SessionHandler.deductFromChannel (or MppServer.deductFromChannel) explicitly when
        // they want to bill a unit. Auto-deducting here would silently double-charge anyone
        // who also tracks billing in their own layer.
        SessionHandler.VoucherAck ack = handler.acceptVoucher(channelId, cum, sig);
        return new SessionResult.VoucherResult(channelId, ack.acceptedCumulativeAmount(),
            ack.idempotent(), BigInteger.ZERO, 0L);
    }

    private SessionResult handleClose(Credential credential) {
        JsonNode payload = credential.payload();
        String channelId = requireText(payload, "channelId", "close.channelId");
        BigInteger cum = parseBigInt(payload, "cumulativeAmount", "close.cumulativeAmount");
        Receipt.SessionReceipt r = handler.close(channelId, cum);
        return new SessionResult.CloseResult(r);
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    private static String requireText(JsonNode node, String key, String human) {
        if (node == null || !node.hasNonNull(key)) {
            throw new InvalidPayloadError("missing required field: " + human);
        }
        return node.path(key).asText();
    }

    private static BigInteger parseBigInt(JsonNode node, String key, String human) {
        try {
            return new BigInteger(requireText(node, key, human));
        } catch (NumberFormatException e) {
            throw new InvalidPayloadError(
                "field " + human + " is not a valid integer: " + e.getMessage());
        }
    }

    private static String requireNonNull(String v, String human) {
        if (v == null || v.isBlank()) {
            throw new InvalidPayloadError("missing required field: " + human);
        }
        return v;
    }

}
