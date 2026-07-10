package com.okx.payments.mpp.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * Server-issued payment challenge — opaque envelope that travels in the
 * {@code WWW-Authenticate: Payment} 402 response and back in the client's
 * {@code Authorization: Payment} retry.
 *
 * <p>{@code id} is HMAC-SHA256(secretKey, realm|method|intent|request|expires|digest|opaque),
 * recomputed at verify time for stateless provenance.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record Challenge(
    @JsonProperty("id") String id,
    @JsonProperty("realm") String realm,
    @JsonProperty("method") Method method,
    @JsonProperty("intent") Intent intent,
    @JsonProperty("request") String request,
    @JsonProperty("expires") String expires,
    @JsonProperty("description") String description,
    @JsonProperty("digest") String digest,
    @JsonProperty("opaque") String opaque
) {

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private String id;
        private String realm;
        private Method method;
        private Intent intent;
        private String request;
        private String expires;
        private String description;
        private String digest;
        private String opaque;

        public Builder id(String v) { this.id = v; return this; }
        public Builder realm(String v) { this.realm = v; return this; }
        public Builder method(Method v) { this.method = v; return this; }
        public Builder intent(Intent v) { this.intent = v; return this; }
        public Builder request(String v) { this.request = v; return this; }
        public Builder expires(String v) { this.expires = v; return this; }
        public Builder description(String v) { this.description = v; return this; }
        public Builder digest(String v) { this.digest = v; return this; }
        public Builder opaque(String v) { this.opaque = v; return this; }
        public Challenge build() {
            return new Challenge(id, realm, method, intent, request, expires, description, digest, opaque);
        }
    }
}
