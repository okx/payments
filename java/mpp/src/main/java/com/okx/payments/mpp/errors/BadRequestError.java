package com.okx.payments.mpp.errors;

/** SA codes 30001 INCORRECT_PARAMETER + 70000 invalid_params. */
public final class BadRequestError extends MppError {
    public BadRequestError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "bad-request"; }
    @Override public String title() { return "Bad Request"; }
    @Override public int status() { return 400; }
    @Override public Integer code() { return 70000; }
}
