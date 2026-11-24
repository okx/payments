package com.okx.payments.mpp.errors;
public final class PaymentRequiredError extends MppError {
    public PaymentRequiredError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "payment-required"; }
    @Override public String title() { return "Payment Required"; }
    @Override public int status() { return 402; }
}
