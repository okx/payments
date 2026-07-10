package com.okx.payments.mpp.errors;
public final class VoucherDeltaTooSmallError extends MppError {
    public VoucherDeltaTooSmallError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "voucher-delta-too-small"; }
    @Override public String title() { return "Voucher Delta Too Small"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70013; }
}
