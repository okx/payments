package com.okx.payments.mpp.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.databind.JsonNode;

/**
 * Client-supplied payment credential — base64url-encoded body of
 * {@code Authorization: Payment <b64>}.
 *
 * <p>{@code payload} is intentionally a raw JsonNode: the SDK dispatches on
 * {@code payload.type} (charge) or {@code payload.action} (session) to deserialize
 * into the concrete payload record. {@code challenge} is null on topUp/settle/close
 * (these are merchant-initiated, not 402-driven).
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record Credential(
    @JsonProperty("challenge") Challenge challenge,
    @JsonProperty("payload") JsonNode payload,
    @JsonProperty("source") String source
) {
}
