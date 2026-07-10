// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.server.PaymentHooks;
import com.okx.x402.server.X402Response;
import com.okx.x402.subscription.eip712.SubscriptionEip712;
import com.okx.x402.subscription.error.SubscriptionErrorCodes;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.PermitDetails;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import com.okx.x402.subscription.model.resp.CreateResp;
import com.okx.x402.subscription.server.store.InMemorySubscriptionStore;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.server.X402Request;
import com.okx.x402.util.Json;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;
import org.web3j.utils.Numeric;

import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.HashMap;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.ArgumentMatchers.*;
import static org.mockito.Mockito.*;

class SubscriptionSchemeHandlerTest {

    private static final String SUBSCRIPTION_CONTRACT = "0x1234567890abcdef1234567890abcdef12345678";

    private SubscriptionFacilitatorClient facilitator;
    private InMemorySubscriptionStore store;
    private SubscriptionSchemeHandler handler;

    @BeforeEach
    void setUp() {
        facilitator = Mockito.mock(SubscriptionFacilitatorClient.class);
        store = new InMemorySubscriptionStore();
        handler = new SubscriptionSchemeHandler(facilitator, store, SUBSCRIPTION_CONTRACT);
    }

    @Test
    void subscribeHappyPath() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.txHash = "0xtx1";
        resp.state = 1;
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        X402Request request = mockRequest(terms, permit);
        X402Response response = mockResponse();

        PaymentHooks.ProtectedRequestResult result = handler.handle(request, response);

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        verify(response).setHeader(eq("X-APP-PAYMENT-RESPONSE"), any());
        StoredSubscription stored = store.get("0xabc123");
        assertNotNull(stored);
        assertEquals(1, stored.state);
    }

    @Test
    void preCheckRejectsMissingFields() throws Exception {
        SubscriptionTerms terms = new SubscriptionTerms();
        PermitSingle permit = new PermitSingle();
        permit.details = new PermitDetails();

        String error = handler.runPreChecks(terms, permit);
        assertEquals("missing_required_terms_fields", error);
        verifyNoInteractions(facilitator);
    }

    @Test
    void preCheckRejectsNullPermitDetails() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = new PermitSingle();
        permit.details = null;
        permit.spender = SUBSCRIPTION_CONTRACT;

        String error = handler.runPreChecks(terms, permit);
        assertEquals("missing_required_terms_fields", error);
    }

    @Test
    void preCheckRejectsInvalidAddress() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        terms.payer = "0xNOTHEX";
        PermitSingle permit = buildMatchingPermit(terms);

        String error = handler.runPreChecks(terms, permit);
        assertEquals("invalid_address_format", error);
    }

    @Test
    void preCheckRejectsInvalidBytes32() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        terms.salt = "0x123";
        PermitSingle permit = buildMatchingPermit(terms);

        String error = handler.runPreChecks(terms, permit);
        assertEquals("invalid_bytes32", error);
    }

    @Test
    void preCheckRejectsTokenMismatch() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.details.token = "0x9999999999999999999999999999999999999999";

        String error = handler.runPreChecks(terms, permit);
        assertEquals("token_mismatch", error);
    }

    @Test
    void preCheckRejectsPermitSpenderMismatch() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.spender = "0x9999999999999999999999999999999999999999";
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        String error = handler.runPreChecks(terms, permit);
        assertEquals("permit_spender_mismatch", error);
    }

    @Test
    void preCheckRejectsNullSpender() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.spender = null;

        String error = handler.runPreChecks(terms, permit);
        assertEquals("permit_spender_mismatch", error);
    }

    @Test
    void preCheckRejectsPermitHashMismatch() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        terms.permitHash = "0x" + "ff".repeat(32);
        PermitSingle permit = buildMatchingPermit(terms);

        String error = handler.runPreChecks(terms, permit);
        assertEquals("permit_hash_mismatch", error);
    }

    @Test
    void preCheckRejectsInitialChargePeriodsExceedMax() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        terms.initialChargePeriods = 13;
        terms.maxPeriods = 12;
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        String error = handler.runPreChecks(terms, permit);
        assertEquals(SubscriptionErrorCodes.INITIAL_CHARGE_PERIODS_EXCEED_MAX, error);
    }

    @Test
    void preCheckRejectsCreateWithNonZeroChangeFrom() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        terms.changeFromSubId = "0x" + "ab".repeat(32);
        terms.changeEffectiveAt = 0;
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        String error = handler.runPreChecks(terms, permit);
        assertEquals(SubscriptionErrorCodes.CREATE_MUST_HAVE_ZERO_CHANGE_FROM, error);
    }

    @Test
    void noHeaderReturnsNull() throws Exception {
        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-PAYMENT")).thenReturn(null);
        X402Response response = mockResponse();

        PaymentHooks.ProtectedRequestResult result = handler.handle(request, response);
        assertNull(result);
    }

    @Test
    void preCheckPassesDeadlineBoundary() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        long now = System.currentTimeMillis() / 1000;
        terms.termsDeadline = now; // equals current time — still valid (> not >=)
        PermitSingle permit = buildMatchingPermit(terms);
        permit.sigDeadline = String.valueOf(now + 1000);
        // Compute permitHash AFTER all permit fields are finalized
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        String error = handler.runPreChecks(terms, permit);
        assertNull(error);
    }

    // ------------------------------------------------------------------
    // Negative tests: input hardening, offer binding, scheduler seeding, change offers
    // ------------------------------------------------------------------

    @Test
    void preCheckAcceptsUint256SigDeadline() throws Exception {
        // A "never expires" Permit2 deadline beyond 2^63-1 must parse and pass.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.sigDeadline = "18446744073709551616"; // 2^64
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        assertNull(handler.runPreChecks(terms, permit));
    }

    @Test
    void preCheckRejectsMalformedSigDeadline() throws Exception {
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.sigDeadline = "not-a-number";

        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));

        permit.sigDeadline = null;
        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));
    }

    @Test
    void settleSeedsUnknownAnchorAndChargeWatermark() throws Exception {
        // storedFromTerms must NOT guess billingAnchorAt / lastChargedPeriod —
        // both stay unknown (0 / null) until an authoritative detail backfill.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.state = 1; // ACTIVE — detail refresh mocked to fail (getSubscription → null)
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        handler.handle(mockRequest(terms, permit), mockResponse());

        StoredSubscription stored = store.get("0xabc123");
        assertEquals(0L, stored.billingAnchorAt);
        assertNull(stored.lastChargedPeriod);
    }

    @Test
    void nonPendingSubscribeRefreshesSchedulerFields() throws Exception {
        // A syncSettle immediate-ACTIVE subscribe still refreshes detail once so
        // nextChargeableAt is seeded and the renewal scheduler can see the subscription.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.state = 1; // ACTIVE, not PENDING → no poll loop
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        com.okx.x402.subscription.model.resp.QueryResp detail =
                new com.okx.x402.subscription.model.resp.QueryResp();
        detail.subId = "0xabc123";
        detail.state = 1;
        detail.payer = terms.payer;
        detail.lastChargedPeriod = 1;
        detail.startAt = terms.startAt;
        detail.billingAnchorAt = terms.startAt;
        detail.periodSec = terms.periodSec;
        detail.maxPeriods = terms.maxPeriods;
        detail.nextChargeableAt = terms.startAt + terms.periodSec;
        when(facilitator.getSubscription("0xabc123")).thenReturn(detail);

        handler.handle(mockRequest(terms, permit), mockResponse());

        StoredSubscription stored = store.get("0xabc123");
        assertEquals((Long) (terms.startAt + terms.periodSec), stored.nextChargeableAt);
        assertEquals((Long) 1L, stored.lastChargedPeriod);
        assertEquals(terms.startAt, stored.billingAnchorAt);
    }

    @Test
    void offerBindingTreatsAbsentPeriodModeAsFixed() {
        // An extra without a periodMode key defaults to fixed_seconds: it must match
        // periodMode=0 terms and still reject periodMode=1 terms.
        SubscriptionTerms terms = buildValidTerms();
        java.util.List<com.okx.x402.server.AcceptOption> accepts =
                java.util.List.of(offerOptionFor(terms, /*includePeriodMode=*/false));

        assertNull(SubscriptionSchemeHandler.verifyTermsMatchOffer(terms, accepts));

        terms.periodMode = 1;
        assertEquals("terms_period_mode_mismatch",
                SubscriptionSchemeHandler.verifyTermsMatchOffer(terms, accepts));
    }

    @Test
    void routeLevelPayToBackfillsIntoOfferBinding() throws Exception {
        // payTo declared at route level (option-level null) must reach the binding via
        // effectiveAccepts() — otherwise it false-rejects with terms_merchant_mismatch.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        com.okx.x402.server.AcceptOption option = offerOptionFor(terms, true);
        option.payTo = null; // route-level default below
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.payTo = terms.merchant;
        config.accepts = new java.util.ArrayList<>(java.util.List.of(option));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.state = 1;
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        PaymentHooks.ProtectedRequestResult result =
                handler.handle(mockRequest(terms, permit), mockResponse(), config);

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
    }

    @Test
    void acceptedNetworkMustMatchRouteAccepts() throws Exception {
        // The buyer's accepted echo cannot pick a network the route does not advertise.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        com.okx.x402.server.AcceptOption option = offerOptionFor(terms, true);
        option.network = "eip155:196";
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>(java.util.List.of(option));

        Map<String, Object> wrapped = new HashMap<>();
        wrapped.put("x402Version", 2);
        wrapped.put("accepted", Map.of("scheme", "period", "network", "eip155:999"));
        wrapped.put("payload", Map.of(
                "terms", Json.MAPPER.valueToTree(terms),
                "permitSingle", Json.MAPPER.valueToTree(permit),
                "termsSignature", "0xsig1",
                "permitSingleSignature", "0xsig2"));
        String b64 = Base64.getEncoder().encodeToString(
                Json.MAPPER.writeValueAsBytes(wrapped));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(b64);

        PaymentHooks.ProtectedRequestResult result =
                handler.handle(request, mockResponse(), config);

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("payment does not match any accepted payment option", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.PAYMENT_FAILED,
                result.abortClass);
        verifyNoInteractions(facilitator);
    }

    @Test
    void changeOffersDegradeToSubscribeOffersOnInvalidProof() throws Exception {
        // An invalid APP-Access proof on the CHANGE route answers the plain subscribe
        // offers (writer wired by the processor), not a bare error.
        String[] captured = new String[1];
        handler.subscribeOfferWriter((req, resp, cfg, err) -> captured[0] = err);

        X402Request request = mock(X402Request.class);
        when(request.getHeader("APP-Access")).thenReturn("%%%not-base64%%%");
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>();

        boolean handled = handler.handleChangeOffers(request, mockResponse(), config);

        assertTrue(handled);
        assertEquals("invalid access proof", captured[0]);
    }

    @Test
    void changeAcceptsCarryAtomicAmountAndTimeout() {
        // Change-offer entries resolve "$" prices to atomic units and carry
        // maxTimeoutSeconds, like every other 402 accepts entry.
        com.okx.x402.server.AcceptOption target = new com.okx.x402.server.AcceptOption();
        target.scheme = "period";
        target.network = "eip155:196";
        target.payTo = "0x2222222222222222222222222222222222222222";
        target.price = "$0.01";
        target.extra = Map.of("plan", Map.of("id", "plan_pro", "tier", 2));

        java.util.List<Map<String, Object>> out = SubscriptionSchemeHandler.buildChangeAccepts(
                java.util.List.of(target), "0xfromsub", "plan_basic", 1);

        assertEquals(1, out.size());
        assertEquals("10000", out.get(0).get("amount")); // $0.01 @ 6 decimals
        assertEquals(86400, out.get(0).get("maxTimeoutSeconds"));
        @SuppressWarnings("unchecked")
        Map<String, Object> extra = (Map<String, Object>) out.get(0).get("extra");
        @SuppressWarnings("unchecked")
        Map<String, Object> changeFrom = (Map<String, Object>) extra.get("changeFrom");
        assertEquals("upgrade", changeFrom.get("direction"));
    }

    // ------------------------------------------------------------------
    // Negative tests: chainIndex binding, detail merge, merchant veto,
    // input hardening, change-accepts builder
    // ------------------------------------------------------------------

    @Test
    void buyerChainIndexCannotOverrideRouteNetwork() throws Exception {
        // A wrapped payload echoing a legal accepted.network must not be able to steer
        // settlement onto another chain via an explicit chainIndex field — the route wins.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        com.okx.x402.server.AcceptOption option = offerOptionFor(terms, true); // eip155:196
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>(java.util.List.of(option));

        Map<String, Object> wrapped = new HashMap<>();
        wrapped.put("x402Version", 2);
        wrapped.put("chainIndex", 999); // buyer-supplied, must be ignored
        wrapped.put("accepted", Map.of("scheme", "period", "network", "eip155:196"));
        wrapped.put("payload", Map.of(
                "terms", Json.MAPPER.valueToTree(terms),
                "permitSingle", Json.MAPPER.valueToTree(permit),
                "termsSignature", "0xsig1",
                "permitSingleSignature", "0xsig2"));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(wrapped)));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.state = 1;
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        handler.handle(request, mockResponse(), config);

        org.mockito.ArgumentCaptor<Long> chain = org.mockito.ArgumentCaptor.forClass(Long.class);
        verify(facilitator).subscribe(chain.capture(), any(), any(), any(), any(), eq(true));
        assertEquals(196L, chain.getValue());
    }

    @Test
    void unresolvedChainIndexAbortsBeforeFacilitator() throws Exception {
        // A payload with no resolvable chain must abort locally with a
        // structured 402 instead of shipping chainIndex=0 to the facilitator.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        Map<String, Object> flat = new HashMap<>(); // legacy flat shape, chainIndex absent
        flat.put("terms", Json.MAPPER.valueToTree(terms));
        flat.put("permit", Json.MAPPER.valueToTree(permit));
        flat.put("termsSignature", "0xsig1");
        flat.put("permitSignature", "0xsig2");
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(flat)));

        PaymentHooks.ProtectedRequestResult result = handler.handle(request, mockResponse());

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("unsupported network for subscription", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.PAYMENT_FAILED,
                result.abortClass);
        verifyNoInteractions(facilitator);
    }

    @Test
    void rehydratePreservesSeededPlanOnSparseDetail() throws Exception {
        // A detail response that omits planId/planTier/payer (or sends planId:"") must
        // not clobber the values seeded from the buyer-signed terms — otherwise a paid
        // subscriber starts failing the plan gate after the very first refresh.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        CreateResp resp = new CreateResp();
        resp.subId = "0xabc123";
        resp.state = 1; // non-pending → rehydrateFromDetail path
        when(facilitator.subscribe(anyLong(), any(), any(), any(), any(), eq(true))).thenReturn(resp);

        com.okx.x402.subscription.model.resp.QueryResp detail =
                new com.okx.x402.subscription.model.resp.QueryResp();
        detail.subId = "0xabc123";
        detail.state = 1;
        detail.payer = "";      // omitted by backend
        detail.planId = "";     // omitted by backend
        detail.planTier = null; // omitted by backend
        detail.lastChargedPeriod = 1;
        detail.startAt = terms.startAt;
        detail.billingAnchorAt = terms.startAt;
        detail.periodSec = terms.periodSec;
        detail.nextChargeableAt = terms.startAt + terms.periodSec;
        when(facilitator.getSubscription("0xabc123")).thenReturn(detail);

        handler.handle(mockRequest(terms, permit), mockResponse());

        StoredSubscription stored = store.get("0xabc123");
        assertEquals(terms.planId, stored.planId);
        assertEquals(terms.planTier, stored.planTier);
        assertEquals(terms.payer, stored.payer);
        assertEquals((Long) 1L, stored.lastChargedPeriod); // on-chain facts still applied
    }

    @Test
    void changeOffersMerchantVetoAnswersBare402() throws Exception {
        // The merchant veto fires BEFORE the period gate on the CHANGE route and answers
        // a bare 402 — never change offers, never subscribe-offer degradation.
        org.web3j.crypto.ECKeyPair keyPair = org.web3j.crypto.Keys.createEcKeyPair();
        String payer = "0x" + org.web3j.crypto.Keys.getAddress(keyPair.getPublicKey());
        String subId = "0x" + "ab".repeat(32);

        StoredSubscription sub = new StoredSubscription();
        sub.subId = subId;
        sub.payer = payer;
        sub.planId = "plan_basic";
        sub.planTier = 1;
        sub.lastChargedPeriod = null; // period gate would fail → degradation, were it reached
        store.put(sub);

        handler.onBeforeAccess((proof, s) ->
                SubscriptionHooks.AccessDecision.deny("banned_by_merchant"));
        boolean[] degraded = {false};
        handler.subscribeOfferWriter((req, resp, cfg, err) -> degraded[0] = true);

        com.okx.x402.subscription.model.AccessProof proof =
                new com.okx.x402.subscription.model.AccessProof();
        proof.subId = subId;
        proof.payer = payer;
        proof.timestamp = System.currentTimeMillis() / 1000;
        byte[] msgHash = com.okx.x402.subscription.eip712.AccessProofEip191.personalSignHash(
                com.okx.x402.subscription.eip712.AccessProofEip191.innerHash(
                        proof.subId, proof.payer, proof.timestamp));
        org.web3j.crypto.Sign.SignatureData sig =
                org.web3j.crypto.Sign.signMessage(msgHash, keyPair, false);
        byte[] sigBytes = new byte[65];
        System.arraycopy(sig.getR(), 0, sigBytes, 0, 32);
        System.arraycopy(sig.getS(), 0, sigBytes, 32, 32);
        sigBytes[64] = sig.getV()[0];
        proof.signature = Numeric.toHexString(sigBytes);

        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-Access")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(proof)));
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>();
        X402Response response = mockResponse();

        boolean handled = handler.handleChangeOffers(request, response, config);

        assertTrue(handled);
        assertFalse(degraded[0]);
        verify(response).setStatus(402);
        verify(response).writeBody(argThat(body -> body.contains("banned_by_merchant")));
        verifyNoInteractions(facilitator); // vetoed before any period-gate detail fetch
    }

    @Test
    void preCheckRejectsOversizedSigDeadline() throws Exception {
        // A sigDeadline ≥ 2^256 must be a structured reject, not an uncaught
        // RuntimeException out of the 32-byte pad in hashPermitSingle (container 500).
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.sigDeadline = java.math.BigInteger.TWO.pow(257).toString();

        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));
    }

    @Test
    void decodeRejectsNullTermsInPayload() throws Exception {
        // A payload with terms:null (or missing permitSingle/signatures) must answer a
        // structured 402, not NPE out of the filter as a 500.
        Map<String, Object> wrapped = new HashMap<>();
        wrapped.put("x402Version", 2);
        wrapped.put("accepted", Map.of("scheme", "period", "network", "eip155:196"));
        Map<String, Object> payload = new HashMap<>();
        payload.put("terms", null);
        payload.put("permitSingle", Json.MAPPER.valueToTree(
                buildMatchingPermit(buildValidTerms())));
        payload.put("termsSignature", "0xsig1");
        payload.put("permitSingleSignature", "0xsig2");
        wrapped.put("payload", payload);
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(wrapped)));

        PaymentHooks.ProtectedRequestResult result = handler.handle(request, mockResponse());

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("invalid subscription payment header", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.PAYMENT_FAILED,
                result.abortClass);
        verifyNoInteractions(facilitator);
    }

    @Test
    void preCheckRejectsNonDecimalAndNegativeFields() throws Exception {
        // Non-decimal amount strings and negative numeric fields are structured
        // rejects (NFE-500 hardening + kills the "-1 sentinel" offer-binding collision).
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.amountPerPeriod = "abc";
        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));

        terms = buildValidTerms();
        permit = buildMatchingPermit(terms);
        permit.details.amount = "0x5";
        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));

        terms = buildValidTerms();
        permit = buildMatchingPermit(terms);
        terms.planTier = -1;
        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));
    }

    @Test
    void changeAcceptsSkipUnresolvableEntriesAndCarryRegistryExtra() {
        // Change accepts follow the unified builder rules — registry EIP-712 extra is
        // merged in, and an unresolvable price is warn-and-skip (never a raw "$" on the wire).
        com.okx.x402.server.AcceptOption good = new com.okx.x402.server.AcceptOption();
        good.scheme = "period";
        good.network = "eip155:196";
        good.payTo = "0x2222222222222222222222222222222222222222";
        good.price = "$0.01";
        good.extra = Map.of("plan", Map.of("id", "plan_pro", "tier", 2));

        com.okx.x402.server.AcceptOption bad = new com.okx.x402.server.AcceptOption();
        bad.scheme = "period";
        bad.network = "eip155:424242"; // no registry asset → unresolvable
        bad.payTo = "0x2222222222222222222222222222222222222222";
        bad.price = "$0.02";
        bad.extra = Map.of("plan", Map.of("id", "plan_max", "tier", 3));

        java.util.List<Map<String, Object>> out = SubscriptionSchemeHandler.buildChangeAccepts(
                java.util.List.of(good, bad), "0xfromsub", "plan_basic", 1);

        assertEquals(1, out.size()); // bad entry skipped, good one intact
        assertEquals("10000", out.get(0).get("amount"));
        @SuppressWarnings("unchecked")
        Map<String, Object> extra = (Map<String, Object>) out.get(0).get("extra");
        assertEquals("1", extra.get("version"));       // registry EIP-712 extra merged
        assertEquals("eip3009", extra.get("transferMethod"));
        assertNotNull(extra.get("changeFrom"));
    }

    @Test
    void pendingChangeDoesNotMarkOldSubChangedLocally() throws Exception {
        // The old subscription's CHANGED state is chain truth, never a local guess at
        // settle time — a still-PENDING upgrade can fail on-chain, and a mis-marked old row
        // would starve its renewal scheduling (dueSubscriptions filters on ACTIVE).
        String oldSubId = "0x" + "ab".repeat(32);
        StoredSubscription oldSub = new StoredSubscription();
        oldSub.subId = oldSubId;
        oldSub.state = 1; // ACTIVE
        store.put(oldSub);

        SubscriptionTerms terms = buildValidTerms();
        terms.changeFromSubId = oldSubId;
        terms.changeEffectiveAt = 1; // immediate upgrade
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        com.okx.x402.subscription.model.resp.ChangeResp resp =
                new com.okx.x402.subscription.model.resp.ChangeResp();
        resp.newSubId = "0xnew123";
        resp.txHash = "0xtx";
        resp.state = 0; // PENDING — outcome not yet known
        when(facilitator.change(anyLong(), eq(oldSubId), any(), any(), any(), any(), eq(true)))
                .thenReturn(resp);

        com.okx.x402.subscription.model.resp.QueryResp newDetail =
                new com.okx.x402.subscription.model.resp.QueryResp();
        newDetail.subId = "0xnew123";
        newDetail.state = 1;
        newDetail.lastChargedPeriod = 1;
        newDetail.elapsedPeriods = 1; // poll early-stops on first attempt
        when(facilitator.getSubscription("0xnew123")).thenReturn(newDetail);
        com.okx.x402.subscription.model.resp.QueryResp oldDetail =
                new com.okx.x402.subscription.model.resp.QueryResp();
        oldDetail.subId = oldSubId;
        oldDetail.state = 1; // chain says the old sub is STILL ACTIVE
        when(facilitator.getSubscription(oldSubId)).thenReturn(oldDetail);

        handler.handle(mockRequest(terms, permit), mockResponse());

        assertEquals(1, store.get(oldSubId).state); // not locally forced to CHANGED
        assertNull(store.get(oldSubId).changedToSubId);
    }

    @Test
    void changeOffersServedWhenAccessAndPaymentHeadersBothPresent() throws Exception {
        // APP-Access takes precedence on the CHANGE route — a payment header alongside it
        // must not short-circuit to the execute path.
        boolean[] degraded = {false};
        handler.subscribeOfferWriter((req, resp, cfg, err) -> degraded[0] = true);

        X402Request request = mock(X402Request.class);
        when(request.getHeader("APP-Access")).thenReturn("%%%not-base64%%%");
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn("somepayment");
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>();

        boolean handled = handler.handleChangeOffers(request, mockResponse(), config);

        assertTrue(handled);      // access proof wins — never falls through to the execute path
        assertTrue(degraded[0]);  // invalid proof degrades to subscribe offers
        verifyNoInteractions(facilitator);
    }

    @Test
    void preCheckRejectsNullOrGarbagePermitToken() {
        // permit.details.token feeds padAddress inside hashPermitSingle — null or
        // non-hex must be a structured reject, never an NPE/NFE out of the encoder (500).
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        permit.details.token = null;
        assertEquals(SubscriptionErrorCodes.INVALID_ADDRESS_FORMAT,
                handler.runPreChecks(terms, permit));

        terms = buildValidTerms();
        permit = buildMatchingPermit(terms);
        permit.details.token = "hello";
        assertEquals(SubscriptionErrorCodes.INVALID_ADDRESS_FORMAT,
                handler.runPreChecks(terms, permit));
    }

    @Test
    void preCheckRejectsOversizedAmounts() {
        // Digits-only values past the uint width must reject before the 32-byte pad
        // in the EIP-712 encoders throws (container 500). Permit2 amount is uint160.
        String tooBig = "1" + "0".repeat(78); // 10^78 > 2^256

        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.amountPerPeriod = tooBig;
        assertEquals(SubscriptionErrorCodes.AMOUNT_PER_PERIOD_INVALID,
                handler.runPreChecks(terms, permit));

        terms = buildValidTerms();
        permit = buildMatchingPermit(terms);
        terms.initialChargeAmount = tooBig;
        assertEquals(SubscriptionErrorCodes.AMOUNT_PER_PERIOD_INVALID,
                handler.runPreChecks(terms, permit));

        terms = buildValidTerms();
        permit = buildMatchingPermit(terms);
        permit.details.amount = java.math.BigInteger.TWO.pow(161).toString();
        assertEquals(SubscriptionErrorCodes.MISSING_REQUIRED_TERMS_FIELDS,
                handler.runPreChecks(terms, permit));
    }

    @Test
    void wrappedPayloadWithoutAcceptedIsRejected() throws Exception {
        // The wrapped payload's accepted node is required — a payload omitting it
        // must not fall back to the buyer's explicit chainIndex (chain-steer vector). Only the
        // legacy FLAT shape may omit accepted.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        Map<String, Object> wrapped = new HashMap<>();
        wrapped.put("x402Version", 2);
        wrapped.put("chainIndex", 999); // buyer-supplied; must never reach the facilitator
        wrapped.put("payload", Map.of(
                "terms", Json.MAPPER.valueToTree(terms),
                "permitSingle", Json.MAPPER.valueToTree(permit),
                "termsSignature", "0xsig1",
                "permitSingleSignature", "0xsig2"));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(wrapped)));

        PaymentHooks.ProtectedRequestResult result = handler.handle(request, mockResponse());

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("invalid subscription payment header", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.PAYMENT_FAILED,
                result.abortClass);
        verifyNoInteractions(facilitator);
    }

    @Test
    void unparseableRouteNetworkRejectsBuyerChainIndex() throws Exception {
        // The route-side chainIndex write-back is UNCONDITIONAL — a misconfigured
        // option network ("eip155:0xc4" → 0) must reject locally, not let the buyer's
        // explicit chainIndex survive the override and ship to the facilitator.
        SubscriptionTerms terms = buildValidTerms();
        PermitSingle permit = buildMatchingPermit(terms);
        terms.permitHash = "0x" + Numeric.toHexStringNoPrefix(
                SubscriptionEip712.hashPermitSingle(permit));

        com.okx.x402.server.AcceptOption option = offerOptionFor(terms, true);
        option.network = "eip155:0xc4"; // not a numeric chain id → chainIndexOf = 0
        com.okx.x402.server.PaymentProcessor.RouteConfig config =
                new com.okx.x402.server.PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>(java.util.List.of(option));

        Map<String, Object> wrapped = new HashMap<>();
        wrapped.put("x402Version", 2);
        wrapped.put("chainIndex", 999);
        wrapped.put("accepted", Map.of("scheme", "period", "network", "eip155:0xc4"));
        wrapped.put("payload", Map.of(
                "terms", Json.MAPPER.valueToTree(terms),
                "permitSingle", Json.MAPPER.valueToTree(permit),
                "termsSignature", "0xsig1",
                "permitSingleSignature", "0xsig2"));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(wrapped)));

        PaymentHooks.ProtectedRequestResult result =
                handler.handle(request, mockResponse(), config);

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("unsupported network for subscription", result.reason);
        verifyNoInteractions(facilitator);
    }

    @Test
    void looksLikeSubscriptionPaymentPeeksSchemeAndShape() throws Exception {
        // Mixed-route dispatch peeks the payment's scheme (accepted.scheme first,
        // then the terms-node shape); no header and undecodable input both delegate.
        X402Request none = mock(X402Request.class);
        assertTrue(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(none));

        X402Request exact = mock(X402Request.class);
        Map<String, Object> exactWrapped = Map.of(
                "x402Version", 2,
                "accepted", Map.of("scheme", "exact", "network", "eip155:196"),
                "payload", Map.of("authorization", Map.of(), "signature", "0xsig"));
        when(exact.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(exactWrapped)));
        assertFalse(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(exact));

        X402Request period = mock(X402Request.class);
        Map<String, Object> periodWrapped = Map.of(
                "x402Version", 2,
                "accepted", Map.of("scheme", "period", "network", "eip155:196"),
                "payload", Map.of("terms", Map.of()));
        when(period.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(periodWrapped)));
        assertTrue(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(period));

        // Legacy flat shape: no accepted echo — the terms node marks it as subscription.
        X402Request flat = mock(X402Request.class);
        when(flat.getHeader("PAYMENT-SIGNATURE")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(Map.of("terms", Map.of()))));
        assertTrue(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(flat));

        // Exact payment arriving on X-PAYMENT only must also fall through to the generic path.
        X402Request xPayment = mock(X402Request.class);
        when(xPayment.getHeader("X-PAYMENT")).thenReturn(Base64.getEncoder()
                .encodeToString(Json.MAPPER.writeValueAsBytes(exactWrapped)));
        assertFalse(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(xPayment));

        X402Request garbage = mock(X402Request.class);
        when(garbage.getHeader("PAYMENT-SIGNATURE")).thenReturn("%%%not-base64%%%");
        assertTrue(SubscriptionSchemeHandler.looksLikeSubscriptionPayment(garbage));
    }

    /** Offer extra matching {@link #buildValidTerms()} verbatim (create shape, icp=1). */
    private com.okx.x402.server.AcceptOption offerOptionFor(SubscriptionTerms terms,
                                                            boolean includePeriodMode) {
        Map<String, Object> extra = new HashMap<>();
        extra.put("plan", Map.of("id", terms.planId, "tier", terms.planTier));
        extra.put("amountPerPeriod", terms.amountPerPeriod);
        extra.put("periodSec", terms.periodSec);
        extra.put("maxPeriods", terms.maxPeriods);
        extra.put("initialCharge", Map.of(
                "periodCount", terms.initialChargePeriods,
                "totalAmount", terms.initialChargeAmount));
        if (includePeriodMode) {
            extra.put("periodMode", terms.periodMode);
        }
        com.okx.x402.server.AcceptOption option = new com.okx.x402.server.AcceptOption();
        option.scheme = "period";
        option.network = "eip155:196";
        option.payTo = terms.merchant;
        option.extra = extra;
        return option;
    }

    private SubscriptionTerms buildValidTerms() {
        SubscriptionTerms t = new SubscriptionTerms();
        t.payer = "0x1111111111111111111111111111111111111111";
        t.merchant = "0x2222222222222222222222222222222222222222";
        t.facilitator = "0x3333333333333333333333333333333333333333";
        t.token = "0x4444444444444444444444444444444444444444";
        t.amountPerPeriod = "5000000";
        t.periodSec = 2592000;
        t.maxPeriods = 12;
        t.startAt = 1700000000L;
        t.initialChargePeriods = 1;
        t.initialChargeAmount = "5000000";
        long now = System.currentTimeMillis() / 1000;
        t.termsDeadline = now + 86400;
        t.permitHash = "0x" + "aa".repeat(32);
        t.salt = "0x" + "bb".repeat(32);
        t.planTier = 2;
        t.changeFromSubId = "0x" + "00".repeat(32);
        t.changeEffectiveAt = 0;
        t.periodMode = 0;
        t.planId = "0x" + "cc".repeat(32);
        return t;
    }

    private PermitSingle buildMatchingPermit(SubscriptionTerms terms) {
        PermitSingle p = new PermitSingle();
        p.details = new PermitDetails();
        p.details.token = terms.token;
        p.details.amount = "60000000";
        p.details.expiration = 1731136000L;
        p.details.nonce = 42;
        p.spender = "0x1234567890abcdef1234567890abcdef12345678";
        p.sigDeadline = String.valueOf(System.currentTimeMillis() / 1000 + 86400);
        return p;
    }

    private X402Request mockRequest(SubscriptionTerms terms, PermitSingle permit) throws Exception {
        SubscriptionSchemeHandler.SubscriptionPayment payment =
                new SubscriptionSchemeHandler.SubscriptionPayment();
        payment.chainIndex = 196;
        payment.terms = terms;
        payment.permit = permit;
        payment.termsSignature = "0xsig1";
        payment.permitSignature = "0xsig2";

        String json = Json.MAPPER.writeValueAsString(payment);
        String b64 = Base64.getEncoder().encodeToString(json.getBytes(StandardCharsets.UTF_8));

        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-PAYMENT")).thenReturn(b64);
        return request;
    }

    private X402Response mockResponse() {
        return mock(X402Response.class);
    }
}
