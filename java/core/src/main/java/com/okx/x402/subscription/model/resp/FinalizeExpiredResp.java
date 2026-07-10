// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;

/** POST /subscriptions/finalize-expired response: {subId, txHash, state}. */
@JsonIgnoreProperties(ignoreUnknown = true)
public class FinalizeExpiredResp {
    public String subId;
    public String txHash;
    public Integer state;
}
