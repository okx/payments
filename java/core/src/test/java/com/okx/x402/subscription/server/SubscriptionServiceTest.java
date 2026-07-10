// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.subscription.error.SubscriptionException;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.enums.SubscriptionState;
import com.okx.x402.subscription.model.resp.*;
import com.okx.x402.subscription.server.store.InMemorySubscriptionStore;
import com.okx.x402.subscription.server.store.StoredSubscription;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;

import java.io.IOException;
import java.util.concurrent.atomic.AtomicReference;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

class SubscriptionServiceTest {

    private SubscriptionFacilitatorClient facilitator;
    private InMemorySubscriptionStore store;
    private SubscriptionService service;

    @BeforeEach
    void setUp() {
        facilitator = Mockito.mock(SubscriptionFacilitatorClient.class);
        store = new InMemorySubscriptionStore();
        service = new SubscriptionService(facilitator, store);
    }

    @Test
    void chargeNowSuccess() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        ChargeResp mockResp = new ChargeResp();
        mockResp.subId = "0xsub1";
        mockResp.period = 3;
        mockResp.state = 1;
        mockResp.planChangeTriggered = false;
        when(facilitator.charge("0xsub1", true)).thenReturn(mockResp);

        ChargeResp result = service.chargeNow("0xsub1");
        assertEquals("0xsub1", result.subId);
        assertEquals(3, result.period);
        assertEquals(SubscriptionState.ACTIVE.getValue(), store.get("0xsub1").state);
    }

    @Test
    void chargeNowPlanChangeTriggered() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xold";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        ChargeResp chargeResp = new ChargeResp();
        chargeResp.subId = "0xold";
        chargeResp.period = 5;
        chargeResp.state = 1;
        chargeResp.planChangeTriggered = true;
        chargeResp.newSubId = "0xnew";
        when(facilitator.charge("0xold", true)).thenReturn(chargeResp);

        QueryResp newSubQuery = new QueryResp();
        newSubQuery.subId = "0xnew";
        newSubQuery.state = SubscriptionState.ACTIVE.getValue();
        when(facilitator.getSubscription("0xnew")).thenReturn(newSubQuery);

        ChargeResp result = service.chargeNow("0xold");
        assertTrue(result.planChangeTriggered);

        assertEquals(SubscriptionState.CHANGED.getValue(), store.get("0xold").state);
        assertEquals("0xnew", store.get("0xold").changedToSubId);
        assertNotNull(store.get("0xnew"));
    }

    @Test
    void chargeNowSelfHealSubscriptionNotActive() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        when(facilitator.charge(eq("0xsub1"), anyBoolean()))
                .thenThrow(new SubscriptionException("subscription_not_active", "not active"));

        QueryResp latest = new QueryResp();
        latest.subId = "0xsub1";
        latest.state = SubscriptionState.CANCELED.getValue();
        latest.changedToSubId = null;
        when(facilitator.getSubscription("0xsub1")).thenReturn(latest);

        AtomicReference<String> failedSubId = new AtomicReference<>();
        service.onChargeFailure((subId, reason) -> failedSubId.set(subId));

        ChargeResp result = service.chargeNow("0xsub1");
        assertEquals("0xsub1", result.subId);
        assertEquals(SubscriptionState.CANCELED.getValue(), store.get("0xsub1").state);
        assertEquals("0xsub1", failedSubId.get());
    }

    @Test
    void chargeNowSelfHealPreservesPayerField() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        sub.payer = "0xbuyer";
        sub.merchant = "0xmerchant";
        sub.token = "0xtoken";
        store.put(sub);

        when(facilitator.charge(eq("0xsub1"), anyBoolean()))
                .thenThrow(new SubscriptionException("subscription_not_active", "not active"));

        QueryResp latest = new QueryResp();
        latest.subId = "0xsub1";
        latest.state = SubscriptionState.CANCELED.getValue();
        when(facilitator.getSubscription("0xsub1")).thenReturn(latest);

        service.chargeNow("0xsub1");

        StoredSubscription healed = store.get("0xsub1");
        assertNotNull(healed.payer, "payer must not be null after self-heal");
        assertEquals("0xbuyer", healed.payer);
        assertEquals("0xmerchant", healed.merchant);
        assertEquals("0xtoken", healed.token);
    }

    @Test
    void chargeNowSelfHealWithChangedToSubId() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xold";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        when(facilitator.charge(eq("0xold"), anyBoolean()))
                .thenThrow(new SubscriptionException("subscription_not_active", "changed"));

        QueryResp latestOld = new QueryResp();
        latestOld.subId = "0xold";
        latestOld.state = SubscriptionState.CHANGED.getValue();
        latestOld.changedToSubId = "0xnew";
        when(facilitator.getSubscription("0xold")).thenReturn(latestOld);

        QueryResp latestNew = new QueryResp();
        latestNew.subId = "0xnew";
        latestNew.state = SubscriptionState.ACTIVE.getValue();
        when(facilitator.getSubscription("0xnew")).thenReturn(latestNew);

        service.chargeNow("0xold");

        assertEquals(SubscriptionState.CHANGED.getValue(), store.get("0xold").state);
        assertNotNull(store.get("0xnew"));
        assertEquals(SubscriptionState.ACTIVE.getValue(), store.get("0xnew").state);
    }

    @Test
    void chargeNowPeriodNotDueIdempotent() throws Exception {
        when(facilitator.charge(eq("0xsub1"), anyBoolean()))
                .thenThrow(new SubscriptionException("period_not_due", "not due"));

        ChargeResp result = service.chargeNow("0xsub1");
        assertEquals("0xsub1", result.subId);
    }

    @Test
    void chargeNowChargeInFlightIdempotent() throws Exception {
        when(facilitator.charge(eq("0xsub1"), anyBoolean()))
                .thenThrow(new SubscriptionException("charge_in_flight", "in flight"));

        ChargeResp result = service.chargeNow("0xsub1");
        assertEquals("0xsub1", result.subId);
    }

    @Test
    void chargeNowAllPeriodsChargedIdempotent() throws Exception {
        when(facilitator.charge(eq("0xsub1"), anyBoolean()))
                .thenThrow(new SubscriptionException("all_periods_charged", "done"));

        ChargeResp result = service.chargeNow("0xsub1");
        assertEquals("0xsub1", result.subId);
    }

    @Test
    void cancelNow() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        CancelAuth auth = new CancelAuth();
        auth.action = 0;
        auth.subId = "0xsub1";
        auth.initiator = 1;

        CancelResp resp = new CancelResp();
        resp.subId = "0xsub1";
        resp.state = 3;
        when(facilitator.cancel("0xsub1", auth, true)).thenReturn(resp);
        // The store row is refreshed from chain truth, not locally guessed.
        QueryResp detail = new QueryResp();
        detail.subId = "0xsub1";
        detail.state = SubscriptionState.CANCELED.getValue();
        when(facilitator.getSubscription("0xsub1")).thenReturn(detail);

        service.cancelNow("0xsub1", auth);
        assertEquals(SubscriptionState.CANCELED.getValue(), store.get("0xsub1").state);
    }

    @Test
    void cancelPendingNow() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        sub.pendingChange = new StoredSubscription.PendingChange();
        sub.pendingChange.newSubId = "0xnew";
        store.put(sub);

        PendingChangeCancelAuth auth = new PendingChangeCancelAuth();
        auth.subId = "0xsub1";

        CancelPendingResp resp = new CancelPendingResp();
        resp.subId = "0xsub1";
        when(facilitator.cancelPendingChange("0xsub1", auth, true)).thenReturn(resp);

        service.cancelPendingNow("0xsub1", auth);
        assertNull(store.get("0xsub1").pendingChange);
    }

    @Test
    void finalizeExpiredNow() throws Exception {
        StoredSubscription sub = new StoredSubscription();
        sub.subId = "0xsub1";
        sub.state = SubscriptionState.ACTIVE.getValue();
        store.put(sub);

        FinalizeExpiredResp resp = new FinalizeExpiredResp();
        resp.subId = "0xsub1";
        when(facilitator.finalizeExpired("0xsub1")).thenReturn(resp);
        // The store row is refreshed from chain truth, not locally guessed.
        QueryResp detail = new QueryResp();
        detail.subId = "0xsub1";
        detail.state = SubscriptionState.COMPLETED.getValue();
        when(facilitator.getSubscription("0xsub1")).thenReturn(detail);

        service.finalizeExpiredNow("0xsub1");
        assertEquals(SubscriptionState.COMPLETED.getValue(), store.get("0xsub1").state);
    }
}
