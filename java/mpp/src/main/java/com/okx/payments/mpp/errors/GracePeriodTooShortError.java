package com.okx.payments.mpp.errors;
public final class GracePeriodTooShortError extends MppError {
    public GracePeriodTooShortError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "grace-period-too-short"; }
    @Override public String title() { return "Grace Period Too Short"; }
    @Override public int status() { return 400; }
    @Override public Integer code() { return 70011; }
}
