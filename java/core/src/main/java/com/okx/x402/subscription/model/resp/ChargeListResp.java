// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

/** GET /subscriptions/charges?subId=&limit=&offset= response (create_time DESC). */
@JsonIgnoreProperties(ignoreUnknown = true)
public class ChargeListResp {
    /** Defaults to empty so a {} data payload cannot NPE callers. */
    public List<ChargeItem> charges = new java.util.ArrayList<>();
    public int total;

    @JsonIgnoreProperties(ignoreUnknown = true)
    public static class ChargeItem {
        public String subId;
        public long period;
        /** 1 initial / 2 periodic / 3 downgrade_first_period / 4 finalize_expired_marker. */
        public int chargeType;
        public String amount;
        public int state;
        public String txHash;
        @JsonProperty("planChangeTriggered")
        public Boolean planChangeTriggered;
        public String newSubId;
    }
}
