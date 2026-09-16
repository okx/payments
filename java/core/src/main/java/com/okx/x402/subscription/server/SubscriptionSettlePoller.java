// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server;

import com.okx.x402.subscription.facilitator.SubscriptionFacilitatorClient;
import com.okx.x402.subscription.model.enums.SubscriptionState;
import com.okx.x402.subscription.model.resp.QueryResp;

import java.io.IOException;
import java.util.Objects;

/**
 * Pending-state poller: when subscribe / change / charge returns
 * {@code state == pending(0)}, poll the public on-chain status endpoint
 * {@code GET /subscriptions/detail?subId=} at a 1-second interval, at most 5 attempts by default,
 * stopping early once {@code lastChargedPeriod >= elapsedPeriods && elapsedPeriods > 0} (i.e. the
 * current wall-clock period is confirmed paid on-chain).
 */
public class SubscriptionSettlePoller {

    public static final long DEFAULT_INTERVAL_MS = 1000;
    public static final int DEFAULT_MAX_ATTEMPTS = 5;

    private final SubscriptionFacilitatorClient facilitator;
    private final long intervalMs;
    private final int maxAttempts;

    public SubscriptionSettlePoller(SubscriptionFacilitatorClient facilitator) {
        this(facilitator, DEFAULT_INTERVAL_MS, DEFAULT_MAX_ATTEMPTS);
    }

    public SubscriptionSettlePoller(SubscriptionFacilitatorClient facilitator,
                                    long intervalMs, int maxAttempts) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.intervalMs = intervalMs;
        this.maxAttempts = maxAttempts;
    }

    /**
     * Stop condition: the current wall-clock period is confirmed paid. Prefers the backend's
     * elapsedPeriods; when an older backend omits it (0), falls back to computing it locally
     * from the returned terms.
     */
    public static boolean settled(QueryResp resp) {
        if (resp == null) {
            return false;
        }
        long elapsed = resp.elapsedPeriods;
        if (elapsed <= 0) {
            long now = System.currentTimeMillis() / 1000;
            long effectiveStart = resp.startAt > 0 ? resp.startAt : now;
            elapsed = com.okx.x402.subscription.support.PeriodMath.elapsedPeriods(
                    resp.periodMode, effectiveStart, resp.billingAnchorAt, resp.periodSec, now);
        }
        return elapsed > 0 && resp.lastChargedPeriod >= elapsed;
    }

    /** Terminal failure — no point polling further. */
    public static boolean failed(QueryResp resp) {
        return resp != null && resp.state == SubscriptionState.FAILED.getValue();
    }

    /**
     * Poll until settled, terminally FAILED, or attempts are exhausted. Returns the LAST detail
     * response (callers re-check {@link #settled}/{@link #failed} to know which way it ended);
     * null only if every poll errored.
     */
    public QueryResp poll(String subId) throws InterruptedException {
        QueryResp last = null;
        for (int attempt = 0; attempt < maxAttempts; attempt++) {
            if (attempt > 0) {
                Thread.sleep(intervalMs);
            }
            try {
                last = facilitator.getSubscription(subId);
            } catch (InterruptedException e) {
                throw e;
            } catch (IOException | RuntimeException e) {
                continue;
            }
            if (settled(last) || failed(last)) {
                return last;
            }
        }
        return last;
    }
}
