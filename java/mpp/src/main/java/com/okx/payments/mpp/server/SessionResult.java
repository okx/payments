package com.okx.payments.mpp.server;

import com.okx.payments.mpp.protocol.Receipt;

import java.math.BigInteger;

/**
 * Discriminated result of {@link EvmSessionMethod#verifySession}. One concrete subtype
 * per session action — open / topUp / voucher / close.
 *
 * <p>Mirrors the rust {@code mpp::session_method::SessionResult} discriminated union.
 */
public sealed interface SessionResult {

    /** Convenience accessor — every variant carries a channelId (post-resolve). */
    String channelId();

    /** Action name for logging / observability ({@code open|topUp|voucher|close}). */
    String action();

    record OpenResult(Receipt.SessionReceipt receipt) implements SessionResult {
        @Override public String channelId() { return receipt.channelId(); }
        @Override public String action()    { return "open"; }
    }

    record TopUpResult(Receipt.SessionReceipt receipt) implements SessionResult {
        @Override public String channelId() { return receipt.channelId(); }
        @Override public String action()    { return "topUp"; }
    }

    /**
     * Voucher acceptance + per-unit deduct. The deduct amount is taken from the original
     * challenge's {@code SessionRequest.amount} (per-unit price) — mirrors Rust.
     */
    record VoucherResult(
        String channelId,
        BigInteger acceptedCumulativeAmount,
        boolean idempotent,
        BigInteger spent,
        long units
    ) implements SessionResult {
        @Override public String action() { return "voucher"; }
    }

    record CloseResult(Receipt.SessionReceipt receipt) implements SessionResult {
        @Override public String channelId() { return receipt.channelId(); }
        @Override public String action()    { return "close"; }
    }
}
