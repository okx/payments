package com.okx.payments.mpp.protocol.charge;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

/**
 * EIP-3009 ReceiveWithAuthorization payload — wire shape for both
 * primary charge and split entries.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record Eip3009Authorization(
    @JsonProperty("type") String type,           // "eip-3009"
    @JsonProperty("from") String from,
    @JsonProperty("to") String to,
    @JsonProperty("value") String value,
    @JsonProperty("validAfter") String validAfter,
    @JsonProperty("validBefore") String validBefore,
    @JsonProperty("nonce") String nonce,
    @JsonProperty("signature") String signature,
    @JsonProperty("splits") List<Eip3009Split> splits
) {
    public static final String TYPE_EIP_3009 = "eip-3009";
    public static final String TYPE_DELEGATION = "delegation";  // P2 — verify rejects

    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Eip3009Split(
        @JsonProperty("from") String from,
        @JsonProperty("to") String to,
        @JsonProperty("value") String value,
        @JsonProperty("validAfter") String validAfter,
        @JsonProperty("validBefore") String validBefore,
        @JsonProperty("nonce") String nonce,
        @JsonProperty("signature") String signature
    ) {}
}
