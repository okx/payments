// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

public class ChargeResp {
    public String subId;
    /** Nullable: a primitive default 0 would fabricate "period 0" on a partial response. */
    public Integer period;
    public String txHash;
    /** Nullable so a missing field is detectable: a primitive default 0 = pending, a LEGAL value. */
    public Integer state;
    public Boolean planChangeTriggered;
    public String newSubId;
}
