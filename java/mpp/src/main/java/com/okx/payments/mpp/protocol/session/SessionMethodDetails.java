package com.okx.payments.mpp.protocol.session;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionMethodDetails(
    @JsonProperty("chainId") Long chainId,
    @JsonProperty("escrowContract") String escrowContract,
    @JsonProperty("feePayer") Boolean feePayer,
    @JsonProperty("minVoucherDelta") String minVoucherDelta,
    @JsonProperty("channelId") String channelId
) {
}
