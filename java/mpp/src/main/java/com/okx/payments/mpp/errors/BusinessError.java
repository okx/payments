package com.okx.payments.mpp.errors;
public final class BusinessError extends MppError {
    public BusinessError(String detail) { super(detail); }
    public BusinessError(String detail, Throwable cause) { super(detail, cause); }
    @Override public String type() { return TYPE_PREFIX + "business-error"; }
    @Override public String title() { return "Business Error"; }
    @Override public int status() { return 500; }
}
