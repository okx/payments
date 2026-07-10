package com.okx.payments.mpp.errors;
public final class ServiceError extends MppError {
    public ServiceError(String detail) { super(detail); }
    public ServiceError(String detail, Throwable cause) { super(detail, cause); }
    @Override public String type() { return TYPE_PREFIX + "service-error"; }
    @Override public String title() { return "Service Error"; }
    @Override public int status() { return 500; }
    @Override public Integer code() { return 8000; }
}
