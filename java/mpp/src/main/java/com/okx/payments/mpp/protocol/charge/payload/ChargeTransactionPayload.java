package com.okx.payments.mpp.protocol.charge.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.okx.payments.mpp.protocol.charge.Eip3009Authorization;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record ChargeTransactionPayload(
    @JsonProperty("type") String type,             // "transaction"
    @JsonProperty("authorization") Eip3009Authorization authorization
) {
    public static final String TYPE = "transaction";
}
