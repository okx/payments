package com.okx.payments.mpp.errors;
public final class UnsupportedChainError extends MppError {
    public UnsupportedChainError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "unsupported-chain"; }
    @Override public String title() { return "Unsupported Chain"; }
    @Override public int status() { return 400; }
    @Override public Integer code() { return 70001; }
}
