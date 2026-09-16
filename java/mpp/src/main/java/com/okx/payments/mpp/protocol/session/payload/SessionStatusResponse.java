package com.okx.payments.mpp.protocol.session.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/** GET /session/status response. */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionStatusResponse(
    @JsonProperty("channelId") String channelId,
    @JsonProperty("payer") String payer,
    @JsonProperty("payee") String payee,
    @JsonProperty("token") String token,
    @JsonProperty("deposit") String deposit,
    @JsonProperty("settledOnChain") String settledOnChain,
    @JsonProperty("sessionStatus") String sessionStatus,    // OPEN | CLOSING | CLOSED
    @JsonProperty("remainingBalance") String remainingBalance
) {
}
