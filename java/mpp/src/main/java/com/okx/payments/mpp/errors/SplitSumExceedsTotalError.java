package com.okx.payments.mpp.errors;
public final class SplitSumExceedsTotalError extends MppError {
    public SplitSumExceedsTotalError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "split-sum-exceeds-total"; }
    @Override public String title() { return "Split Sum Exceeds Total"; }
    @Override public int status() { return 400; }
    @Override public Integer code() { return 70005; }
}
