// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.access;

import com.okx.x402.server.AcceptOption;
import com.okx.x402.server.PaymentHooks;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Request;
import com.okx.x402.subscription.eip712.AccessProofEip191;
import com.okx.x402.subscription.error.SubscriptionErrorCodes;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.AccessProof;
import com.okx.x402.subscription.model.resp.QueryResp;
import com.okx.x402.subscription.server.SubscriptionHooks;
import com.okx.x402.subscription.server.store.InMemorySubscriptionStore;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.util.Json;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;
import org.web3j.crypto.Sign;
import org.web3j.utils.Numeric;

import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.List;
import java.util.Map;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

/**
 * Period judgment: two-level lastChargedPeriod vs elapsed comparison
 * (never isActive/state); plan gate against the route's accepts list; merchant
 * on_before_access short-circuit.
 */
class AccessProofHookPeriodTest {

    private static final String SUB_ID = "0x" + "ab".repeat(32);
    private static final long PERIOD_SEC = 2_592_000L;

    private ECKeyPair keyPair;
    private String payer;
    private InMemorySubscriptionStore store;
    private SubscriptionFacilitatorClient facilitator;
    private AccessProofHook hook;

    @BeforeEach
    void setUp() throws Exception {
        keyPair = Keys.createEcKeyPair();
        payer = "0x" + Keys.getAddress(keyPair.getPublicKey());
        store = new InMemorySubscriptionStore();
        facilitator = Mockito.mock(SubscriptionFacilitatorClient.class);
        hook = new AccessProofHook(store, facilitator);
    }

    private StoredSubscription putSub(long startAt, long lastChargedPeriod) {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = SUB_ID;
        sub.payer = payer;
        sub.periodSec = PERIOD_SEC;
        sub.periodMode = 0;
        sub.maxPeriods = 12;
        sub.startAt = startAt;
        sub.billingAnchorAt = startAt;
        sub.lastChargedPeriod = lastChargedPeriod;
        sub.updatedAt = System.currentTimeMillis() / 1000; // fresh within the access-cache TTL
        sub.planId = "plan_basic";
        sub.planTier = 1;
        store.put(sub);
        return sub;
    }

    private X402Request requestWithProof() throws Exception {
        AccessProof proof = new AccessProof();
        proof.subId = SUB_ID;
        proof.payer = payer;
        proof.timestamp = System.currentTimeMillis() / 1000;

        byte[] msgHash = AccessProofEip191.personalSignHash(
                AccessProofEip191.innerHash(proof.subId, proof.payer, proof.timestamp));
        Sign.SignatureData sig = Sign.signMessage(msgHash, keyPair, false);
        byte[] sigBytes = new byte[65];
        System.arraycopy(sig.getR(), 0, sigBytes, 0, 32);
        System.arraycopy(sig.getS(), 0, sigBytes, 32, 32);
        sigBytes[64] = sig.getV()[0];
        proof.signature = Numeric.toHexString(sigBytes);

        String b64 = Base64.getEncoder().encodeToString(
                Json.MAPPER.writeValueAsString(proof).getBytes(StandardCharsets.UTF_8));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-Access")).thenReturn(b64);
        return request;
    }

    private static PaymentProcessor.RouteConfig routeAccepting(String... planIds) {
        PaymentProcessor.RouteConfig config = new PaymentProcessor.RouteConfig();
        config.accepts = new java.util.ArrayList<>();
        for (String planId : planIds) {
            AcceptOption option = AcceptOption.builder()
                    .scheme("permit2_subscription")
                    .extra(Map.of("plan", Map.of("id", planId, "tier", 1)))
                    .build();
            config.accepts.add(option);
        }
        return config;
    }

    @Test
    void rule1LocalPaidPeriodGrantsWithoutNetwork() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 1); // mid period 1, paid to 1

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        verifyNoInteractions(facilitator);
    }

    @Test
    void rule2RefreshGrantsWhenServerConfirmsPaid() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC - 10, 1); // period 2 entered, store stale at 1

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.state = 1;
        latest.lastChargedPeriod = 2;
        latest.elapsedPeriods = 2;
        latest.startAt = now - PERIOD_SEC - 10;
        latest.billingAnchorAt = now - PERIOD_SEC - 10;
        latest.periodSec = PERIOD_SEC;
        latest.maxPeriods = 12;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        assertEquals(2, store.get(SUB_ID).lastChargedPeriod); // store refreshed
    }

    @Test
    void rule2DeniesWhenPeriodUnpaid() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC - 10, 1);

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.lastChargedPeriod = 1;
        latest.elapsedPeriods = 2; // time outran payment
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(SubscriptionErrorCodes.SUBSCRIPTION_PERIOD_UNPAID, result.reason);
    }

    @Test
    void expiredFullyPaidSubIsDenied() throws Exception {
        // lastCharged == maxPeriods but wall clock is at period 15: local UNCAPPED elapsed (15)
        // beats lastCharged (12) so the hook must go remote and then deny.
        long now = System.currentTimeMillis() / 1000;
        putSub(now - 15 * PERIOD_SEC + PERIOD_SEC / 2, 12);

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.lastChargedPeriod = 12;
        latest.elapsedPeriods = 15;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(SubscriptionErrorCodes.SUBSCRIPTION_PERIOD_UNPAID, result.reason);
    }

    @Test
    void prepaidMultiPeriodGrantsViaGreaterEqual() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 3); // initialChargePeriods=3 style prepay, in period 1

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        verifyNoInteractions(facilitator);
    }

    @Test
    void startAtZeroForcesDetailRefresh() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(0, 1); // startAt never refreshed → local math forbidden

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.lastChargedPeriod = 1;
        latest.elapsedPeriods = 1;
        latest.startAt = now - 100; // backfilled on-chain block time
        latest.billingAnchorAt = now - 100;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        assertEquals(now - 100, store.get(SUB_ID).startAt); // real chain time backfilled
    }

    @Test
    void planNotInAcceptsIsRejectedWithPlanMismatch() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - 100, 1);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_pro"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(SubscriptionErrorCodes.PLAN_MISMATCH, result.reason);
        verifyNoInteractions(facilitator);
    }

    @Test
    void onBeforeAccessDenyShortCircuitsPeriodJudgment() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 1); // would pass rule 1

        hook.onBeforeAccess((proof, sub) ->
                SubscriptionHooks.AccessDecision.deny("merchant_canceled"));

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("merchant_canceled", result.reason);
        verifyNoInteractions(facilitator);
    }

    @Test
    void noHeaderProceeds() {
        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-Access")).thenReturn(null);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(request, routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.PROCEED, result.decision);
    }

    // ------------------------------------------------------------------
    // Negative tests (store-miss rehydrate / abort classes)
    // ------------------------------------------------------------------

    @Test
    void storeMissRehydratesFromFacilitator() throws Exception {
        // Empty store (seller restart) — a valid subscriber must be recovered from the
        // facilitator, not permanently 402'd.
        long now = System.currentTimeMillis() / 1000;
        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.state = 1;
        latest.payer = payer;
        latest.lastChargedPeriod = 1;
        latest.elapsedPeriods = 1;
        latest.startAt = now - 100;
        latest.billingAnchorAt = now - 100;
        latest.periodSec = PERIOD_SEC;
        latest.maxPeriods = 12;
        latest.planId = "plan_basic";
        latest.planTier = 1;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        assertNotNull(store.get(SUB_ID)); // write-through row created
        assertEquals(payer, store.get(SUB_ID).payer);
    }

    @Test
    void storeMissWithFacilitatorDownIsUnauthorizedBare() throws Exception {
        // Rehydrate failed → UNAUTHORIZED (bare 402): never instruct an agent to re-subscribe
        // on a transient outage (double-charge vector).
        when(facilitator.getSubscription(SUB_ID)).thenThrow(new RuntimeException("boom"));

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("subscription not found", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED,
                result.abortClass);
    }

    @Test
    void statusUnavailableOnSlowPathIsUnauthorizedBare() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC - 10, 1); // period 2 entered → slow path required
        when(facilitator.getSubscription(SUB_ID)).thenThrow(new RuntimeException("boom"));

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("subscription status unavailable", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED,
                result.abortClass);
    }

    @Test
    void periodUnpaidIsNotEligibleWithOffers() throws Exception {
        // NOT_ELIGIBLE keeps the offers envelope: (re)subscribing genuinely fixes this.
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC - 10, 1);

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.lastChargedPeriod = 1;
        latest.elapsedPeriods = 2;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.NOT_ELIGIBLE,
                result.abortClass);
    }

    @Test
    void merchantVetoIsDeniedBare() throws Exception {
        // DENIED (bare 402): paying again would not restore a vetoed buyer's access.
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 1);
        hook.onBeforeAccess((proof, sub) ->
                SubscriptionHooks.AccessDecision.deny("merchant_canceled"));

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.DENIED, result.abortClass);
    }

    @Test
    void invalidSignatureIsUnauthorizedBare() throws Exception {
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 1);

        AccessProof proof = new AccessProof();
        proof.subId = SUB_ID;
        proof.payer = payer;
        proof.timestamp = now;
        proof.signature = "0x" + "11".repeat(65); // garbage signature

        String b64 = Base64.getEncoder().encodeToString(
                Json.MAPPER.writeValueAsString(proof).getBytes(StandardCharsets.UTF_8));
        X402Request request = mock(X402Request.class);
        when(request.getHeader("X-APP-Access")).thenReturn(b64);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(request, routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED,
                result.abortClass);
        verifyNoInteractions(facilitator);
    }

    // ------------------------------------------------------------------
    // Veto ordering / payer fall-through / elapsed fallback / verifier hardening
    // ------------------------------------------------------------------

    @Test
    void vetoRunsBeforePlanGate() throws Exception {
        // A vetoed buyer whose plan ALSO mismatches must get the DENIED bare 402, not
        // a plan_mismatch 402 + offers inviting them to pay.
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC / 2, 1); // plan_basic — mismatches the plan_pro route below
        hook.onBeforeAccess((proof, sub) ->
                SubscriptionHooks.AccessDecision.deny("merchant_banned"));

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_pro"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals("merchant_banned", result.reason);
        assertEquals(PaymentHooks.ProtectedRequestResult.AbortClass.DENIED, result.abortClass);
    }

    @Test
    void stalePayerFallsThroughToAuthoritativeCheck() throws Exception {
        // A store row with an unknown/stale payer disqualifies the fast path but is NOT
        // a hard deny — the slow path re-checks ownership on the authoritative detail.
        long now = System.currentTimeMillis() / 1000;
        StoredSubscription sub = putSub(now - PERIOD_SEC / 2, 1);
        sub.payer = "0x9999999999999999999999999999999999999999"; // stale / clobbered
        store.put(sub);

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.state = 1;
        latest.payer = payer; // authoritative payer matches the proof signer
        latest.lastChargedPeriod = 1;
        latest.elapsedPeriods = 1;
        latest.startAt = now - PERIOD_SEC / 2;
        latest.billingAnchorAt = now - PERIOD_SEC / 2;
        latest.periodSec = PERIOD_SEC;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        assertEquals(payer, store.get(SUB_ID).payer); // authoritative payer written through
    }

    @Test
    void slowPathComputesElapsedWhenBackendOmitsIt() throws Exception {
        // An older backend omitting elapsedPeriods (0) must not deny a paid subscriber —
        // the gate falls back to local period math.
        long now = System.currentTimeMillis() / 1000;
        putSub(now - PERIOD_SEC - 10, 1); // period 2 entered, store watermark stale at 1

        QueryResp latest = new QueryResp();
        latest.subId = SUB_ID;
        latest.state = 1;
        latest.payer = payer;
        latest.lastChargedPeriod = 2;   // paid to period 2
        latest.elapsedPeriods = 0;      // omitted by backend
        latest.startAt = now - PERIOD_SEC - 10;
        latest.billingAnchorAt = now - PERIOD_SEC - 10;
        latest.periodSec = PERIOD_SEC;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(latest);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
    }

    @Test
    void wrongKindAndMalformedSignaturesAreRejected() throws Exception {
        // An explicit wrong kind is rejected; a >65-byte signature must never be
        // silently truncated to its first 65 bytes.
        AccessProof proof = new AccessProof();
        proof.subId = SUB_ID;
        proof.payer = payer;
        proof.timestamp = System.currentTimeMillis() / 1000;
        byte[] msgHash = AccessProofEip191.personalSignHash(
                AccessProofEip191.innerHash(proof.subId, proof.payer, proof.timestamp));
        Sign.SignatureData sig = Sign.signMessage(msgHash, keyPair, false);
        byte[] sigBytes = new byte[65];
        System.arraycopy(sig.getR(), 0, sigBytes, 0, 32);
        System.arraycopy(sig.getS(), 0, sigBytes, 32, 32);
        sigBytes[64] = sig.getV()[0];
        proof.signature = Numeric.toHexString(sigBytes);

        AccessProofVerifier verifier = new AccessProofVerifier();
        assertNull(verifier.verify(proof)); // baseline: this proof IS valid

        proof.kind = "payment-id";
        assertEquals("invalid access proof kind", verifier.verify(proof));

        proof.kind = "subscription-id";
        proof.signature = proof.signature + "00"; // 66 bytes — must not truncate-and-verify
        assertEquals("payer mismatch", verifier.verify(proof));
    }

    @Test
    void emptyPlanIdRefreshesFromDetailBeforePlanGate() throws Exception {
        // The plan gate judges refreshed data — a cached
        // row with no planId refreshes from the authoritative detail before judging, so a paid
        // subscriber self-heals once the backend supplies planId instead of being permanently
        // 402'd plan_mismatch on a stale null (with the slow path never reached).
        long now = System.currentTimeMillis() / 1000;
        StoredSubscription sub = putSub(now - 100, 1);
        sub.planId = null; // legacy signing / detail omission at rehydrate time
        store.put(sub);

        QueryResp detail = new QueryResp();
        detail.subId = SUB_ID;
        detail.payer = payer;
        detail.state = 1;
        detail.planId = "plan_basic"; // backend now supplies it
        detail.planTier = 1;
        detail.startAt = now - 100;
        detail.billingAnchorAt = now - 100;
        detail.lastChargedPeriod = 1;
        detail.elapsedPeriods = 1;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(detail);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.GRANT_ACCESS, result.decision);
        assertEquals("plan_basic", store.get(SUB_ID).planId); // refresh wrote through
    }

    @Test
    void emptyPlanIdStillMismatchesWhenDetailNamesAnotherPlan() throws Exception {
        // Negative: the refresh is not a bypass — a backend-supplied planId outside the
        // route's accepts still hits the plan gate.
        long now = System.currentTimeMillis() / 1000;
        StoredSubscription sub = putSub(now - 100, 1);
        sub.planId = null;
        store.put(sub);

        QueryResp detail = new QueryResp();
        detail.subId = SUB_ID;
        detail.payer = payer;
        detail.state = 1;
        detail.planId = "plan_other";
        detail.planTier = 2;
        detail.startAt = now - 100;
        detail.billingAnchorAt = now - 100;
        detail.lastChargedPeriod = 1;
        detail.elapsedPeriods = 1;
        when(facilitator.getSubscription(SUB_ID)).thenReturn(detail);

        PaymentHooks.ProtectedRequestResult result =
                hook.onProtectedRequest(requestWithProof(), routeAccepting("plan_basic"));

        assertEquals(PaymentHooks.ProtectedRequestResult.Decision.ABORT, result.decision);
        assertEquals(SubscriptionErrorCodes.PLAN_MISMATCH, result.reason);
    }
}
