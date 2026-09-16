// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.subscription.error.SubscriptionException;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.enums.ChargeState;
import com.okx.x402.subscription.model.enums.SubscriptionState;
import com.okx.x402.subscription.model.resp.ChargeResp;
import com.okx.x402.subscription.model.resp.QueryResp;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.subscription.server.store.SubscriptionStore;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

public class SubscriptionService {

    private final SubscriptionFacilitatorClient facilitator;
    private final SubscriptionStore store;
    private final SubscriptionSettlePoller poller;
    private final List<SubscriptionHooks.BeforeChargeHook> beforeChargeHooks = new ArrayList<>();
    private final List<SubscriptionHooks.AfterChargeHook> afterChargeHooks = new ArrayList<>();
    private final List<SubscriptionHooks.OnChargeFailureHook> chargeFailureHooks = new ArrayList<>();

    public SubscriptionService(SubscriptionFacilitatorClient facilitator, SubscriptionStore store) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.store = Objects.requireNonNull(store);
        this.poller = new SubscriptionSettlePoller(facilitator);
    }

    public SubscriptionService onBeforeCharge(SubscriptionHooks.BeforeChargeHook hook) {
        beforeChargeHooks.add(hook);
        return this;
    }

    public SubscriptionService onAfterCharge(SubscriptionHooks.AfterChargeHook hook) {
        afterChargeHooks.add(hook);
        return this;
    }

    public SubscriptionService onChargeFailure(SubscriptionHooks.OnChargeFailureHook hook) {
        chargeFailureHooks.add(hook);
        return this;
    }

    public ChargeResp chargeNow(String subId) throws IOException, InterruptedException {
        for (SubscriptionHooks.BeforeChargeHook h : beforeChargeHooks) h.beforeCharge(subId);

        try {
            ChargeResp resp = facilitator.charge(subId, true);

            if (Boolean.TRUE.equals(resp.planChangeTriggered) && resp.newSubId != null) {
                StoredSubscription oldSub = store.get(subId);
                if (oldSub != null) {
                    oldSub.state = SubscriptionState.CHANGED.getValue();
                    oldSub.changedToSubId = resp.newSubId;
                    store.put(oldSub);
                }
                syncFromFacilitator(resp.newSubId, oldSub);
            }

            // Charge state still pending → poll detail (1s × 5, early-stop once the current
            // period is confirmed paid or terminally FAILED) and refresh.
            if (resp.state != null && resp.state == ChargeState.PENDING.getValue()) {
                QueryResp latest = poller.poll(subId);
                if (latest != null) {
                    applyDetail(store.get(subId), latest);
                }
            } else {
                StoredSubscription sub = store.get(subId);
                if (sub != null && resp.period != null && (sub.lastChargedPeriod == null
                        || resp.period > sub.lastChargedPeriod)) {
                    sub.lastChargedPeriod = resp.period.longValue();
                    sub.updatedAt = System.currentTimeMillis() / 1000;
                    store.put(sub);
                }
            }

            int chargedPeriod = resp.period != null ? resp.period : 0;
            for (SubscriptionHooks.AfterChargeHook h : afterChargeHooks) h.afterCharge(subId, chargedPeriod);
            return resp;
        } catch (SubscriptionException e) {
            return handleChargeError(subId, e);
        }
    }

    public void cancelNow(String subId, CancelAuth auth) throws IOException, InterruptedException {
        facilitator.cancel(subId, auth, true);
        // Refresh from chain truth instead of locally guessing CANCELED — the settle may
        // still be pending or partially applied.
        syncFromFacilitator(subId, null);
    }

    public void cancelPendingNow(String subId, PendingChangeCancelAuth auth)
            throws IOException, InterruptedException {
        facilitator.cancelPendingChange(subId, auth, true);
        StoredSubscription sub = store.get(subId);
        if (sub != null && sub.pendingChange != null) {
            sub.pendingChange = null;
            store.put(sub);
        }
        syncFromFacilitator(subId, null);
    }

    public void finalizeExpiredNow(String subId) throws IOException, InterruptedException {
        facilitator.finalizeExpired(subId);
        // Chain truth over a local COMPLETED guess.
        syncFromFacilitator(subId, null);
    }

    public void syncFromChain(String subId) throws IOException, InterruptedException {
        syncFromFacilitator(subId, null);
    }

    /**
     * Active subscriptions due for charge at {@code nowSec}: a store scan keeping ACTIVE
     * records whose nextChargeableAt has arrived. Records with no known next time are skipped
     * until a detail refresh backfills it.
     */
    public List<StoredSubscription> dueSubscriptions(long nowSec) {
        List<StoredSubscription> due = new ArrayList<>();
        for (StoredSubscription sub : store.list()) {
            if (sub.state == SubscriptionState.ACTIVE.getValue()
                    && sub.nextChargeableAt != null && sub.nextChargeableAt <= nowSec) {
                due.add(sub);
            }
        }
        return due;
    }

    /**
     * Charge a subscription and reconcile the store: a normal charge advances
     * nextChargeableAt/state via a detail refresh; a downgrade activation
     * additionally records the successor subId so future charges target it. Wraps
     * {@link #chargeNow} — hooks and idempotent error handling apply unchanged.
     */
    public ChargeResp chargeAndRecord(String subId) throws IOException, InterruptedException {
        ChargeResp resp = chargeNow(subId);
        // chargeNow already refreshed on pending; a settled charge still needs the detail pass
        // to advance nextChargeableAt for the scheduler.
        syncFromFacilitator(subId, null);
        return resp;
    }

    private ChargeResp handleChargeError(String subId, SubscriptionException e)
            throws IOException, InterruptedException {
        String code = e.getCode();
        switch (code) {
            case "subscription_not_active":
                return selfHeal(subId, e);
            case "period_not_due":
            case "charge_in_flight":
            case "all_periods_charged":
                ChargeResp idempotent = new ChargeResp();
                idempotent.subId = subId;
                return idempotent;
            default:
                for (SubscriptionHooks.OnChargeFailureHook h : chargeFailureHooks) {
                    h.onChargeFailure(subId, code);
                }
                throw e;
        }
    }

    private ChargeResp selfHeal(String subId, SubscriptionException original)
            throws IOException, InterruptedException {
        QueryResp latest = facilitator.getSubscription(subId);
        StoredSubscription existing = store.get(subId);
        StoredSubscription stored = existing != null ? existing : new StoredSubscription();
        stored.subId = subId;
        applyDetail(stored, latest);

        if (latest.changedToSubId != null && !latest.changedToSubId.isEmpty()) {
            syncFromFacilitator(latest.changedToSubId, existing);
        }

        for (SubscriptionHooks.OnChargeFailureHook h : chargeFailureHooks) {
            h.onChargeFailure(subId, original.getCode());
        }

        ChargeResp resp = new ChargeResp();
        resp.subId = subId;
        return resp;
    }

    private void syncFromFacilitator(String subId, StoredSubscription source)
            throws IOException, InterruptedException {
        QueryResp query = facilitator.getSubscription(subId);
        StoredSubscription stored = store.get(subId);
        if (stored == null) {
            stored = new StoredSubscription();
            stored.subId = subId;
            if (source != null) {
                stored.payer = source.payer;
                stored.merchant = source.merchant;
                stored.token = source.token;
            }
        }
        applyDetail(stored, query);
    }

    /**
     * Fold the authoritative detail fields into a store row and persist it. Delegates to the
     * canonical {@link StoredSubscription#applyDetail} merge.
     */
    private void applyDetail(StoredSubscription stored, QueryResp latest) {
        if (stored == null || latest == null) {
            return;
        }
        stored.applyDetail(latest);
        store.put(stored);
    }
}
