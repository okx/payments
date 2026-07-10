package com.okx.payments.mpp.errors;
public final class ChannelClosedError extends MppError {
    public ChannelClosedError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "channel-closed"; }
    @Override public String title() { return "Channel Closed"; }
    @Override public int status() { return 410; }
    @Override public Integer code() { return 70008; }
}
