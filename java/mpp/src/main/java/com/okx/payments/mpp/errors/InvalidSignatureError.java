package com.okx.payments.mpp.errors;

/** SA 70004 — voucher / SettleAuth / CloseAuth signature verification failed. */
public final class InvalidSignatureError extends MppError {
    public InvalidSignatureError(String detail) { super(detail); }
    public InvalidSignatureError(String detail, Throwable cause) { super(detail, cause); }
    @Override public String type() { return TYPE_PREFIX + "invalid-signature"; }
    @Override public String title() { return "Invalid Signature"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70004; }
}
