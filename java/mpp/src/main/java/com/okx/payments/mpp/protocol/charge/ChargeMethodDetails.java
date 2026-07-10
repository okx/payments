package com.okx.payments.mpp.protocol.charge;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

/**
 * Method-specific details for an MPP charge request.
 *
 * <p>{@code resourceUrl} is an optional, opaque endpoint identifier that the seller may
 * attach to the challenge so the SA backend can attribute each settle to a specific
 * endpoint for revenue / volume reporting per URL. The SDK does not parse or validate
 * the value — it is base64url-embedded in {@code challenge.request} and forwarded
 * verbatim to {@code POST /charge/settle} and {@code POST /charge/verifyHash}.
 *
 * <p>This field is intentionally <strong>charge-only</strong>. The session intent does
 * not have a counterpart: one session spans multiple endpoints, so a single URL would
 * misattribute revenue across them.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record ChargeMethodDetails(
    @JsonProperty("chainId") Long chainId,
    @JsonProperty("feePayer") Boolean feePayer,
    @JsonProperty("permit2Address") String permit2Address,
    @JsonProperty("memo") String memo,
    @JsonProperty("splits") List<ChargeSplit> splits,
    /**
     * Optional endpoint URL tag for SA-side per-endpoint revenue / volume reporting.
     * Pass any string the seller wants to group on — full URL, path, or a logical name.
     * Pass {@code null} to omit the field (Jackson drops it via {@code NON_NULL}).
     */
    @JsonProperty("resourceUrl") String resourceUrl
) {
    /**
     * Backwards-compatible constructor for the pre-{@code resourceUrl} shape. Defers
     * to the canonical record constructor with {@code resourceUrl = null}.
     */
    public ChargeMethodDetails(Long chainId, Boolean feePayer, String permit2Address,
                               String memo, List<ChargeSplit> splits) {
        this(chainId, feePayer, permit2Address, memo, splits, null);
    }
}
