package com.okx.payments.mpp.errors;
public final class PayerBlockedError extends MppError {
    public PayerBlockedError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "payer-blocked"; }
    @Override public String title() { return "Payer Blocked"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70002; }
}
