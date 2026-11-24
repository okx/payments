package com.okx.payments.mpp.protocol.session.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.okx.payments.mpp.protocol.charge.Eip3009Authorization;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionTopUpPayload(
    @JsonProperty("action") String action,                          // "topUp"
    @JsonProperty("type") String type,                              // "transaction" | "hash"
    @JsonProperty("channelId") String channelId,
    @JsonProperty("authorization") Eip3009Authorization authorization,
    @JsonProperty("signature") String signature,
    @JsonProperty("hash") String hash,
    @JsonProperty("additionalDeposit") String additionalDeposit,
    @JsonProperty("topUpSalt") String topUpSalt
) {
    public static final String ACTION = "topUp";
}
