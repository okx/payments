package com.okx.payments.mpp.errors;
public final class InvalidChallengeError extends MppError {
    public InvalidChallengeError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "invalid-challenge"; }
    @Override public String title() { return "Invalid Challenge"; }
    @Override public int status() { return 402; }
}
