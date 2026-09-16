// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.access;

import com.okx.x402.server.AcceptOption;
import com.okx.x402.server.PaymentHooks;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.X402Request;
import com.okx.x402.subscription.error.SubscriptionErrorCodes;
import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.AccessProof;
import com.okx.x402.subscription.model.enums.PeriodMode;
import com.okx.x402.subscription.model.resp.QueryResp;
import com.okx.x402.subscription.server.SubscriptionHooks;
import com.okx.x402.subscription.server.store.StoredSubscription;
import com.okx.x402.subscription.server.store.SubscriptionStore;
import com.okx.x402.subscription.support.PeriodMath;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

/**
 * Buyer access gate driven by the AccessProof (X-APP-Access) header.
 *
 * <p>Pipeline: signature verify → plan-in-accepts check →
 * merchant on_before_access hook (short-circuit) → period judgment. Liveness is decided purely
 * by comparing "how far time has run" (elapsedPeriods) against "how far money has been paid"
 * (lastChargedPeriod) — never by a {@code state == ACTIVE} check:
 *
 * <ol>
 *   <li>Locally compute {@code currentCalculatePeriod} = UNCAPPED elapsedPeriods from store data.
 *       If {@code lastChargedPeriod >= currentCalculatePeriod && currentCalculatePeriod > 0}
 *       → grant with no network call.</li>
 *   <li>Otherwise fetch /subscriptions/detail, refresh the store (lastChargedPeriod, startAt /
 *       billingAnchorAt backfill for startAt=0 subs, state, changedToSubId), and grant iff the
 *       server-side {@code lastChargedPeriod >= elapsedPeriods && elapsedPeriods > 0}.</li>
 * </ol>
 *
 * <p>Buyer-initiated cancel needs no special-casing: the current (paid) period keeps passing rule
 * 1/2 until its boundary, after which lastChargedPeriod stops advancing and access is denied.
 * Merchant-initiated cancel policy is delegated to {@link SubscriptionHooks.OnBeforeAccessHook}.
 *
 * <p>The local computation MUST use the uncapped elapsed formula — a capped currentPeriod would
 * make an expired, fully-paid subscription (lastCharged == maxPeriods == cap) pass rule 1
 * forever.
 */
public class AccessProofHook implements PaymentHooks.OnProtectedRequestHook {

    /** Default access-cache freshness window (30s). 0 = always go remote. */
    public static final long DEFAULT_ACCESS_CACHE_TTL_SECONDS = 30;

    private final SubscriptionStore store;
    private final SubscriptionFacilitatorClient facilitator;
    private final AccessProofVerifier verifier;
    private final List<SubscriptionHooks.OnBeforeAccessHook> beforeAccessHooks = new ArrayList<>();
    private long accessCacheTtlSeconds = DEFAULT_ACCESS_CACHE_TTL_SECONDS;

    public AccessProofHook(SubscriptionStore store, SubscriptionFacilitatorClient facilitator) {
        this(store, facilitator, AccessProofVerifier.DEFAULT_TIME_WINDOW_SECONDS);
    }

    public AccessProofHook(SubscriptionStore store, SubscriptionFacilitatorClient facilitator,
                           int timeWindowSeconds) {
        this.store = Objects.requireNonNull(store);
        this.facilitator = Objects.requireNonNull(facilitator);
        this.verifier = new AccessProofVerifier(timeWindowSeconds);
    }

    /** Override the access-cache freshness window (secs); 0 disables the local fast path. */
    public AccessProofHook accessCacheTtlSeconds(long seconds) {
        this.accessCacheTtlSeconds = seconds;
        return this;
    }

    /** Register a merchant access-override hook (runs after signature verify, before period math). */
    public AccessProofHook onBeforeAccess(SubscriptionHooks.OnBeforeAccessHook hook) {
        beforeAccessHooks.add(hook);
        return this;
    }

    @Override
    public PaymentHooks.ProtectedRequestResult onProtectedRequest(
            X402Request request, PaymentProcessor.RouteConfig config) {
        // Subscription operation routes (change / cancel / cancel-pending-change) own their
        // access-proof handling in SubscriptionSchemeHandler: on a CHANGE route the proof
        // means "show me my change options", and granting access here would serve the
        // underlying resource instead of the change-offer 402. Cancel routes are dispatched
        // before the hook loop and never reach this.
        if (config != null && config.subscriptionOperation != null) {
            return PaymentHooks.ProtectedRequestResult.proceed();
        }
        String header = AccessProofVerifier.readAccessHeader(request);
        if (header == null || header.isEmpty()) {
            return PaymentHooks.ProtectedRequestResult.proceed();
        }

        AccessProof proof = verifier.decode(header);
        if (proof == null) {
            return PaymentHooks.ProtectedRequestResult.abort("invalid access proof",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }

        String sigError = verifier.verify(proof);
        if (sigError != null) {
            return PaymentHooks.ProtectedRequestResult.abort(sigError,
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }

        StoredSubscription sub = store.get(proof.subId);
        if (sub == null) {
            // The store is a cache, not the source of truth — a miss (seller
            // restart, multi-instance without a shared store) rehydrates from the facilitator
            // before rejecting a possibly-valid subscriber.
            sub = rehydrate(proof.subId);
        }
        if (sub == null) {
            return PaymentHooks.ProtectedRequestResult.abort("subscription not found",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }
        // Merchant override (e.g. seller-canceled subscription policy) fires BEFORE the plan
        // gate: a vetoed buyer gets the DENIED bare 402, never a plan_mismatch
        // 402 + offers inviting them to pay.
        for (SubscriptionHooks.OnBeforeAccessHook hook : beforeAccessHooks) {
            SubscriptionHooks.AccessDecision decision = hook.beforeAccess(proof, sub);
            if (decision != null && decision.denied) {
                return PaymentHooks.ProtectedRequestResult.abort(
                        decision.reason != null ? decision.reason
                                : SubscriptionErrorCodes.ACCESS_DENIED_BY_MERCHANT,
                        PaymentHooks.ProtectedRequestResult.AbortClass.DENIED);
            }
        }

        // Plan gate: buyer's planId must be one of the route's accepted plans. A miss falls
        // through as abort → the processor answers 402 + PAYMENT-REQUIRED with this error code,
        // prompting the buyer to (re)subscribe or /changePlan.
        Set<String> acceptedPlanIds = acceptedPlanIds(config);
        if (!acceptedPlanIds.isEmpty()) {
            // The plan gate judges refreshed data — a cached row
            // with no planId (legacy signing / detail omission at rehydrate time) refreshes from
            // the authoritative detail before judging, so a paid subscriber self-heals once the
            // backend supplies planId instead of being permanently 402'd on a stale null.
            if (sub.planId == null || sub.planId.isEmpty()) {
                QueryResp latest = fetchDetail(proof.subId);
                if (latest != null) {
                    refreshStore(sub, latest);
                }
            }
            if (sub.planId == null || !acceptedPlanIds.contains(sub.planId)) {
                return PaymentHooks.ProtectedRequestResult.abort(
                        SubscriptionErrorCodes.PLAN_MISMATCH);
            }
        }

        // A store row whose payer is unknown or stale is NOT a hard deny —
        // it merely disqualifies the local fast path. The slow path re-checks ownership against
        // the authoritative detail (and skips only when the backend omits payer). Computed after
        // the plan-gate refresh so a just-backfilled payer counts.
        boolean ownerOk = sub.payer != null && sub.payer.equalsIgnoreCase(proof.payer);

        return judgePeriod(sub, proof, ownerOk);
    }

    private PaymentHooks.ProtectedRequestResult judgePeriod(StoredSubscription sub,
                                                            AccessProof proof,
                                                            boolean ownerOk) {
        long now = System.currentTimeMillis() / 1000;

        // Local fast path preconditions:
        // - record fresh within the TTL (bounded staleness);
        // - startAt known (0 = the on-chain block time was never backfilled);
        // - calendar-month subs additionally need the anchor backfilled — falling back to
        //   startAt would drift the month-end rhythm and mis-count the period;
        // - lastChargedPeriod fetched at least once (null = unknown, not "0 = unpaid").
        boolean fresh = accessCacheTtlSeconds > 0
                && now - sub.updatedAt <= accessCacheTtlSeconds;
        boolean anchorKnown = !PeriodMode.isCalendarMonth(sub.periodMode)
                || sub.billingAnchorAt > 0;
        if (ownerOk && fresh && sub.startAt > 0 && anchorKnown && sub.lastChargedPeriod != null) {
            long currentCalculatePeriod = PeriodMath.elapsedPeriods(
                    sub.periodMode, sub.startAt, sub.billingAnchorAt, sub.periodSec, now);
            if (sub.lastChargedPeriod >= currentCalculatePeriod && currentCalculatePeriod > 0) {
                return PaymentHooks.ProtectedRequestResult.grantAccess();
            }
        }

        // Local check failed (store stale, new period entered, or startAt unresolved):
        // fetch the authoritative detail, refresh the store, and judge on server-derived values.
        QueryResp latest;
        try {
            latest = facilitator.getSubscription(sub.subId);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return PaymentHooks.ProtectedRequestResult.abort("interrupted",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        } catch (Exception e) {
            // UNAUTHORIZED (bare 402), NOT an offers 402: a transient facilitator outage must
            // never instruct an agent buyer to re-subscribe (double-charge vector).
            return PaymentHooks.ProtectedRequestResult.abort("subscription status unavailable",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }
        if (latest == null) {
            return PaymentHooks.ProtectedRequestResult.abort("subscription status unavailable",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }

        refreshStore(sub, latest);

        // Ownership recheck on authoritative data: the proof signer must be the
        // subscription's payer; skip only when the backend omits it.
        if (latest.payer != null && !latest.payer.isEmpty()
                && !latest.payer.equalsIgnoreCase(proof.payer)) {
            return PaymentHooks.ProtectedRequestResult.abort("payer mismatch",
                    PaymentHooks.ProtectedRequestResult.AbortClass.UNAUTHORIZED);
        }

        // Shared judgment with the settle poller: prefers the backend's elapsedPeriods and
        // falls back to local period math when an older backend omits it — an omitted
        // elapsedPeriods must never deny a paid subscriber.
        if (com.okx.x402.subscription.server.SubscriptionSettlePoller.settled(latest)) {
            return PaymentHooks.ProtectedRequestResult.grantAccess();
        }
        return PaymentHooks.ProtectedRequestResult.abort(
                SubscriptionErrorCodes.SUBSCRIPTION_PERIOD_UNPAID);
    }

    /**
     * Pull the authoritative on-chain-derived fields into the store. Notably backfills the real
     * startAt / billingAnchorAt for subscriptions signed with startAt=0: once such a
     * subscription activates, the actual on-chain times are recovered via the detail query
     * instead of leaving the stored 0 in place.
     */
    private void refreshStore(StoredSubscription sub, QueryResp latest) {
        // Canonical merge: omitted planId/planTier/anchor fields
        // preserve the prior real value instead of clobbering it — see StoredSubscription#applyDetail.
        sub.applyDetail(latest);
        store.put(sub);
    }

    /**
     * Authoritative store-miss recovery: build a fresh store row from /subscriptions/detail.
     * Returns null when the facilitator is unreachable or the subscription does not exist.
     */
    private StoredSubscription rehydrate(String subId) {
        QueryResp d = fetchDetail(subId);
        if (d == null || d.subId == null || d.subId.isEmpty()) {
            return null;
        }
        StoredSubscription sub = new StoredSubscription();
        sub.subId = d.subId;
        refreshStore(sub, d);
        return sub;
    }

    /** Best-effort authoritative detail fetch; null on outage / unknown subscription. */
    private QueryResp fetchDetail(String subId) {
        try {
            return facilitator.getSubscription(subId);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return null;
        } catch (Exception e) {
            return null;
        }
    }

    private static Set<String> acceptedPlanIds(PaymentProcessor.RouteConfig config) {
        Set<String> planIds = new HashSet<>();
        if (config == null || config.accepts == null) {
            return planIds;
        }
        for (AcceptOption option : config.accepts) {
            Map<String, Object> extra = option.extra;
            if (extra == null) continue;
            Object plan = extra.get("plan");
            if (plan instanceof Map<?, ?> planMap) {
                Object id = planMap.get("id");
                if (id instanceof String s && !s.isEmpty()) {
                    planIds.add(s);
                }
            }
        }
        return planIds;
    }
}
