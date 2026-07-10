package com.okx.payments.mpp.errors;
public final class SplitCountExceededError extends MppError {
    public SplitCountExceededError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "split-count-exceeded"; }
    @Override public String title() { return "Split Count Exceeded"; }
    @Override public int status() { return 400; }
    @Override public Integer code() { return 70006; }
}
