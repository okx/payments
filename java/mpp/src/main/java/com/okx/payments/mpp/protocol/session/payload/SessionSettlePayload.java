package com.okx.payments.mpp.protocol.session.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * /session/settle relayer-mode payload.
 * deadline is uint256-decimal seconds-since-epoch String.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionSettlePayload(
    @JsonProperty("action") String action,            // "settle"
    @JsonProperty("channelId") String channelId,
    @JsonProperty("cumulativeAmount") String cumulativeAmount,
    @JsonProperty("voucherSignature") String voucherSignature,
    @JsonProperty("payeeSignature") String payeeSignature,
    @JsonProperty("nonce") String nonce,
    @JsonProperty("deadline") String deadline
) {
    public static final String ACTION = "settle";
}
