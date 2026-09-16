// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.model.resp;

import com.fasterxml.jackson.annotation.JsonProperty;

public class AllowanceStatusResp {
    public long nonce;
    @JsonProperty("reservedAmount")
    public String reservedAmount;
    @JsonProperty("reservedExpiration")
    public long reservedExpiration;
    @JsonProperty("subscriptionContract")
    public String subscriptionContract;
    @JsonProperty("permit2Contract")
    public String permit2Contract;
    @JsonProperty("permit2Allowance")
    public String permit2Allowance;
}
