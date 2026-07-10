package com.okx.payments.mpp.errors;
public final class PaymentExpiredError extends MppError {
    public PaymentExpiredError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "payment-expired"; }
    @Override public String title() { return "Payment Expired"; }
    @Override public int status() { return 402; }
}
