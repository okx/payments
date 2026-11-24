package com.okx.payments.mpp.errors;
public final class ChallengeInvalidError extends MppError {
    public ChallengeInvalidError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "challenge-invalid"; }
    @Override public String title() { return "Challenge Invalid"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70009; }
}
