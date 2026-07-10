package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.InsufficientBalanceError;

import java.math.BigInteger;
import java.util.Optional;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicReference;

/**
 * Default {@link SessionStore} — in-memory, ConcurrentHashMap of {@link AtomicReference}s
 * for lock-free CAS.
 *
 * <p>WARNING: data is lost on JVM restart. Production deployments should plug in a durable
 * implementation. {@code MppServer} emits a startup WARN log when this default is detected.
 */
public final class InMemorySessionStore implements SessionStore {

    private final ConcurrentHashMap<String, AtomicReference<Channel>> map = new ConcurrentHashMap<>();

    @Override
    public Optional<Channel> load(String channelId) {
        AtomicReference<Channel> ref = map.get(channelId);
        return ref == null ? Optional.empty() : Optional.ofNullable(ref.get());
    }

    @Override
    public void put(Channel c) {
        map.compute(c.channelId(), (k, existing) -> {
            if (existing == null) return new AtomicReference<>(c);
            existing.set(c);
            return existing;
        });
    }

    @Override
    public boolean casLastAccepted(String channelId, BigInteger expected,
                                   BigInteger newAccepted, byte[] newSignature) {
        AtomicReference<Channel> ref = map.get(channelId);
        if (ref == null) return false;
        while (true) {
            Channel cur = ref.get();
            if (cur == null) return false;
            BigInteger curVal = cur.lastAccepted() == null ? BigInteger.ZERO : cur.lastAccepted();
            if (!curVal.equals(expected)) return false;
            Channel next = withLastAccepted(cur, newAccepted, newSignature);
            if (ref.compareAndSet(cur, next)) return true;
        }
    }

    @Override
    public void updateDeposit(String channelId, BigInteger newDeposit) {
        mutate(channelId, c -> new Channel(c.channelId(), c.payer(), c.payee(), c.token(),
            c.escrow(), c.chainId(), c.authorizedSigner(), newDeposit, c.lastAccepted(),
            c.lastVoucherSignature(), c.settledOnChain(), c.status(),
            c.spent(), c.units()));
    }

    @Override
    public void updateSettledOnChain(String channelId, BigInteger newSettledOnChain) {
        mutate(channelId, c -> new Channel(c.channelId(), c.payer(), c.payee(), c.token(),
            c.escrow(), c.chainId(), c.authorizedSigner(), c.deposit(), c.lastAccepted(),
            c.lastVoucherSignature(), newSettledOnChain, c.status(),
            c.spent(), c.units()));
    }

    @Override
    public void markStatus(String channelId, ChannelStatus status) {
        mutate(channelId, c -> new Channel(c.channelId(), c.payer(), c.payee(), c.token(),
            c.escrow(), c.chainId(), c.authorizedSigner(), c.deposit(), c.lastAccepted(),
            c.lastVoucherSignature(), c.settledOnChain(), status,
            c.spent(), c.units()));
    }

    @Override
    public DeductResult deduct(String channelId, BigInteger amount) {
        if (amount == null || amount.signum() <= 0) {
            throw new IllegalArgumentException("deduct amount must be > 0");
        }
        AtomicReference<Channel> ref = map.get(channelId);
        if (ref == null) {
            throw (ChannelNotFoundError) new ChannelNotFoundError(
                "channel not found for deduct: " + channelId).put("channelId", channelId);
        }
        while (true) {
            Channel cur = ref.get();
            if (cur == null) {
                throw (ChannelNotFoundError) new ChannelNotFoundError(
                    "channel not found for deduct: " + channelId).put("channelId", channelId);
            }
            BigInteger available = cur.lastAccepted().subtract(cur.spent());
            if (available.compareTo(amount) < 0) {
                throw (InsufficientBalanceError) new InsufficientBalanceError(
                    "insufficient balance: requested " + amount + " but available " + available)
                    .put("channelId", channelId)
                    .put("available", available.toString())
                    .put("requested", amount.toString());
            }
            BigInteger newSpent = cur.spent().add(amount);
            long newUnits = cur.units() + 1L;
            Channel next = new Channel(cur.channelId(), cur.payer(), cur.payee(), cur.token(),
                cur.escrow(), cur.chainId(), cur.authorizedSigner(),
                cur.deposit(), cur.lastAccepted(), cur.lastVoucherSignature(),
                cur.settledOnChain(), cur.status(),
                newSpent, newUnits);
            if (ref.compareAndSet(cur, next)) {
                return new DeductResult(newSpent, newUnits);
            }
            // CAS lost — retry
        }
    }

    private void mutate(String channelId, java.util.function.Function<Channel, Channel> fn) {
        AtomicReference<Channel> ref = map.get(channelId);
        if (ref == null) return;
        while (true) {
            Channel cur = ref.get();
            if (cur == null) return;
            Channel next = fn.apply(cur);
            if (ref.compareAndSet(cur, next)) return;
        }
    }

    private static Channel withLastAccepted(Channel c, BigInteger newAccepted, byte[] newSig) {
        return new Channel(c.channelId(), c.payer(), c.payee(), c.token(),
            c.escrow(), c.chainId(), c.authorizedSigner(),
            c.deposit(), newAccepted, newSig,
            c.settledOnChain(), c.status(),
            c.spent(), c.units());
    }
}
