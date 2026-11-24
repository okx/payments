package com.okx.payments.mpp.protocol.session.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * /session/close relayer-mode payload.
 *
 * <p>{@code voucherSignature} may be the empty string {@code ""} when the merchant takes the
 * waiver path ({@code cumulativeAmount <= settledOnChain}); otherwise a packed 65-byte
 * EIP-712 signature is required.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionClosePayload(
    @JsonProperty("action") String action,            // "close"
    @JsonProperty("channelId") String channelId,
    @JsonProperty("cumulativeAmount") String cumulativeAmount,
    @JsonProperty("voucherSignature") String voucherSignature,
    @JsonProperty("payeeSignature") String payeeSignature,
    @JsonProperty("nonce") String nonce,
    @JsonProperty("deadline") String deadline
) {
    public static final String ACTION = "close";
}
