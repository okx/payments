package com.okx.payments.mpp.protocol.session;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionRequest(
    @JsonProperty("amount") String amount,
    @JsonProperty("currency") String currency,
    @JsonProperty("recipient") String recipient,
    @JsonProperty("unitType") String unitType,
    @JsonProperty("suggestedDeposit") String suggestedDeposit,
    @JsonProperty("description") String description,
    @JsonProperty("externalId") String externalId,
    @JsonProperty("methodDetails") SessionMethodDetails methodDetails
) {
}
