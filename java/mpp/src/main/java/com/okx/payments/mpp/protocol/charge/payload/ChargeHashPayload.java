package com.okx.payments.mpp.protocol.charge.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

@JsonInclude(JsonInclude.Include.NON_NULL)
public record ChargeHashPayload(
    @JsonProperty("type") String type,    // "hash"
    @JsonProperty("hash") String hash
) {
    public static final String TYPE = "hash";
}
