// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.facilitator;

import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import com.okx.x402.subscription.model.resp.*;

import java.io.IOException;

/**
 * REST client for the OKX facilitator subscription API: write endpoints carry subId in the BODY
 * (AK auth forbids path placeholders); detail is a public
 * {@code GET /subscriptions/detail?subId=}.
 */
public interface SubscriptionFacilitatorClient {

    CreateResp subscribe(long chainIndex, SubscriptionTerms terms, PermitSingle permit,
                         String termsSig, String permitSig,
                         boolean syncSettle) throws IOException, InterruptedException;

    ChargeResp charge(String subId, boolean syncSettle) throws IOException, InterruptedException;

    ChangeResp change(long chainIndex, String oldSubId, SubscriptionTerms newTerms,
                      PermitSingle newPermit, String termsSig, String permitSig,
                      boolean syncSettle) throws IOException, InterruptedException;

    CancelResp cancel(String subId, CancelAuth auth, boolean syncSettle)
            throws IOException, InterruptedException;

    CancelPendingResp cancelPendingChange(String subId, PendingChangeCancelAuth auth,
                                          boolean syncSettle)
            throws IOException, InterruptedException;

    FinalizeExpiredResp finalizeExpired(String subId) throws IOException, InterruptedException;

    QueryResp getSubscription(String subId) throws IOException, InterruptedException;

    ChargeListResp getCharges(String subId, int limit, int offset)
            throws IOException, InterruptedException;

    PendingChangeResp getPendingChange(String subId) throws IOException, InterruptedException;

    AllowanceStatusResp getAllowanceStatus(String buyer, String token, long chainIndex)
            throws IOException, InterruptedException;

    BuyerSubscriptionListResp getBuyerSubscriptions(String buyer, int limit, int offset)
            throws IOException, InterruptedException;
}
