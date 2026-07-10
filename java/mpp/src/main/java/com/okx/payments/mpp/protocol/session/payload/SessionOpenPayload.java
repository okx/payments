package com.okx.payments.mpp.protocol.session.payload;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.okx.payments.mpp.protocol.charge.Eip3009Authorization;

/**
 * /session/open payload — supports {@code type=transaction} (Eip3009 platform-facilitated open)
 * and {@code type=hash} (client already broadcast). v4 removed initial voucher fields.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record SessionOpenPayload(
    @JsonProperty("action") String action,                         // "open"
    @JsonProperty("type") String type,                             // "transaction" | "hash"
    @JsonProperty("channelId") String channelId,
    @JsonProperty("authorization") Eip3009Authorization authorization,  // required when type=transaction
    @JsonProperty("signature") String signature,                   // EIP-3009 sig (transaction)
    @JsonProperty("hash") String hash,                             // open tx hash (hash mode)
    @JsonProperty("salt") String salt,
    @JsonProperty("authorizedSigner") String authorizedSigner
) {
    public static final String ACTION = "open";
    public static final String TYPE_TRANSACTION = "transaction";
    public static final String TYPE_HASH = "hash";
}
