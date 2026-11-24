// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * GET /subscriptions/pending?subId= response — the latest pending-change row (any state, so
 * terminal CANCELED / ACTIVATED / EXPIRED are observable).
 */
@JsonIgnoreProperties(ignoreUnknown = true)
public class PendingChangeResp {
    public String subId;
    public String newSubId;
    @JsonProperty("effectiveFromPeriod")
    public Long effectiveFromPeriod;
    /** Nullable: the "no pending change" body is all-null — a primitive 0 would read as PENDING. */
    public Integer state;
    public int newPlanTier;
    public String fromSubId;
}
