package com.okx.payments.mpp.protocol.charge;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record ChargeRequest(
    @JsonProperty("amount") String amount,
    @JsonProperty("currency") String currency,
    @JsonProperty("recipient") String recipient,
    @JsonProperty("description") String description,
    @JsonProperty("externalId") String externalId,
    @JsonProperty("methodDetails") ChargeMethodDetails methodDetails
) {
}
