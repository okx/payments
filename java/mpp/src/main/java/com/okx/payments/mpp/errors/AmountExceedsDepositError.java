package com.okx.payments.mpp.errors;
public final class AmountExceedsDepositError extends MppError {
    public AmountExceedsDepositError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "amount-exceeds-deposit"; }
    @Override public String title() { return "Amount Exceeds Deposit"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70012; }
}
