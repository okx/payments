package com.okx.payments.mpp.protocol.charge;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record ChargeSplit(
    @JsonProperty("amount") String amount,
    @JsonProperty("recipient") String recipient,
    @JsonProperty("memo") String memo
) {
}
