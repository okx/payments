package com.okx.payments.mpp.errors;
public final class ChannelNotFoundError extends MppError {
    public ChannelNotFoundError(String detail) { super(detail); }
    @Override public String type() { return TYPE_PREFIX + "channel-not-found"; }
    @Override public String title() { return "Channel Not Found"; }
    @Override public int status() { return 410; }
    @Override public Integer code() { return 70010; }
}
