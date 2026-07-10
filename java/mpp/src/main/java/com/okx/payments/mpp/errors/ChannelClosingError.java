package com.okx.payments.mpp.errors;
public final class ChannelClosingError extends MppError {
    public ChannelClosingError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "channel-closing"; }
    @Override public String title() { return "Channel Closing"; }
    @Override public int status() { return 410; }
    @Override public Integer code() { return 70014; }
}
