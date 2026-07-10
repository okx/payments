package com.okx.payments.mpp.errors;

/** SA 70003 invalid_credential — missing source DID, feePayer/hash conflict, etc. */
public final class InvalidPayloadError extends MppError {
    public InvalidPayloadError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "invalid-payload"; }
    @Override public String title() { return "Invalid Payload"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70003; }
}
