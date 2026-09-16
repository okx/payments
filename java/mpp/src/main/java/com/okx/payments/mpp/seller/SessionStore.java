package com.okx.payments.mpp.seller;

import java.math.BigInteger;
import java.util.Optional;

/**
 * Per-channel state owned by the Seller SDK.
 *
 * <p>Default implementation is in-memory; {@code MppServer} logs a WARN at startup when
 * {@link InMemorySessionStore} is in use because mid-session vouchers are lost on restart.
 * Multi-instance deployments and durability across restarts require a custom impl
 * (Redis, JDBC, …).
 */
public interface SessionStore {

    Optional<Channel> load(String channelId);

    void put(Channel channel);

    /**
     * Compare-and-set the highest accepted voucher (atomic).
     *
     * @return {@code true} on success; {@code false} if {@code expected} doesn't match the
     *         current value (caller may retry / read-modify-write).
     */
    boolean casLastAccepted(String channelId, BigInteger expected, BigInteger newAccepted, byte[] newSignature);

    void updateDeposit(String channelId, BigInteger newDeposit);

    void updateSettledOnChain(String channelId, BigInteger newSettledOnChain);

    void markStatus(String channelId, ChannelStatus status);

    /**
     * Atomic per-call billing — mirrors Rust mpp {@code deduct_from_channel}. Computes
     * {@code available = lastAccepted - spent} and either:
     * <ul>
     *   <li>throws {@link com.okx.payments.mpp.errors.InsufficientBalanceError} (70015) when
     *       {@code available < amount} — caller must prompt the client for a higher voucher;</li>
     *   <li>or atomically increments {@code spent += amount} and {@code units += 1}, returning
     *       the post-update {@link DeductResult}.</li>
     * </ul>
     *
     * <p>Implementers MUST keep the read+check+write critical section atomic per
     * {@code channelId}. The default {@link InMemorySessionStore} uses lock-free CAS.
     *
     * @throws com.okx.payments.mpp.errors.ChannelNotFoundError if channel is unknown
     * @throws com.okx.payments.mpp.errors.InsufficientBalanceError when available &lt; amount
     */
    DeductResult deduct(String channelId, BigInteger amount);

    enum ChannelStatus { OPEN, CLOSING, CLOSED }

    /** Post-update billing snapshot returned from {@link #deduct}. */
    record DeductResult(BigInteger spent, long units) {}

    record Channel(
        String channelId,
        String payer,
        String payee,
        String token,
        String escrow,
        long chainId,
        String authorizedSigner,
        BigInteger deposit,
        BigInteger lastAccepted,
        byte[] lastVoucherSignature,
        BigInteger settledOnChain,
        ChannelStatus status,
        /** Total amount already deducted via {@link #deduct}. Invariant: {@code spent <= lastAccepted}. */
        BigInteger spent,
        /** Total billed calls — {@link #deduct} invocation count. */
        long units
    ) {
        public Channel {
            if (channelId == null) throw new IllegalArgumentException("channelId");
            if (payer == null) throw new IllegalArgumentException("payer");
            if (payee == null) throw new IllegalArgumentException("payee");
            if (token == null) throw new IllegalArgumentException("token");
            if (escrow == null) throw new IllegalArgumentException("escrow");
            if (deposit == null) deposit = BigInteger.ZERO;
            if (lastAccepted == null) lastAccepted = BigInteger.ZERO;
            if (settledOnChain == null) settledOnChain = BigInteger.ZERO;
            if (status == null) status = ChannelStatus.OPEN;
            if (spent == null) spent = BigInteger.ZERO;
            // null lastVoucherSignature is fine (no voucher accepted yet)
        }

        /** Convenience constructor — defaults {@code spent=0, units=0}. */
        public Channel(String channelId, String payer, String payee, String token, String escrow,
                       long chainId, String authorizedSigner,
                       BigInteger deposit, BigInteger lastAccepted, byte[] lastVoucherSignature,
                       BigInteger settledOnChain, ChannelStatus status) {
            this(channelId, payer, payee, token, escrow, chainId, authorizedSigner,
                deposit, lastAccepted, lastVoucherSignature, settledOnChain, status,
                BigInteger.ZERO, 0L);
        }
    }
}
