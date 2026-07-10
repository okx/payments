package com.okx.payments.mpp.errors;

/**
 * SDK-local equivalent of Rust mpp's 70015 ("insufficient-balance") — generated when
 * {@code SessionStore.deduct} finds {@code lastAccepted - spent < amount}. Indicates
 * the client must submit a higher cumulative voucher before the next billable call.
 */
public final class InsufficientBalanceError extends MppError {
    public InsufficientBalanceError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "insufficient-balance"; }
    @Override public String title() { return "Insufficient Balance"; }
    @Override public int status() { return 402; }
    @Override public Integer code() { return 70015; }
}
