package com.okx.payments.mpp.errors;
public final class TxNotConfirmedError extends MppError {
    public TxNotConfirmedError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "tx-not-confirmed"; }
    @Override public String title() { return "Transaction Not Confirmed"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70007; }
}
