package com.okx.payments.mpp.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * Server-issued receipt returned in the {@code Payment-Receipt} response header
 * after a successful settle. Sealed: {@link ChargeReceipt} or {@link SessionReceipt}.
 */
public sealed interface Receipt permits Receipt.ChargeReceipt, Receipt.SessionReceipt {

    Method method();
    String status();
    String timestamp();
    Long chainId();
    String reference();

    /** Charge receipt — single tx confirmation (carries challengeId for client correlation). */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    record ChargeReceipt(
        @JsonProperty("method") Method method,
        @JsonProperty("reference") String reference,
        @JsonProperty("status") String status,
        @JsonProperty("timestamp") String timestamp,
        @JsonProperty("chainId") Long chainId,
        @JsonProperty("challengeId") String challengeId,
        @JsonProperty("externalId") String externalId
    ) implements Receipt {}

    /** Session receipt — open/topUp/settle/close. v4: drop challengeId/acceptedCumulative/spent/units; add deposit. */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    record SessionReceipt(
        @JsonProperty("method") Method method,
        @JsonProperty("intent") Intent intent,
        @JsonProperty("status") String status,
        @JsonProperty("timestamp") String timestamp,
        @JsonProperty("channelId") String channelId,
        @JsonProperty("deposit") String deposit,
        @JsonProperty("chainId") Long chainId,
        @JsonProperty("reference") String reference
    ) implements Receipt {}
}
