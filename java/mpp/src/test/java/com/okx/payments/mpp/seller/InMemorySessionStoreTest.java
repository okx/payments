package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.errors.ChannelNotFoundError;
import com.okx.payments.mpp.errors.InsufficientBalanceError;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

import java.math.BigInteger;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.atomic.AtomicInteger;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class InMemorySessionStoreTest {

    private InMemorySessionStore store;

    private static final String CHANNEL = "0x" + "ab".repeat(32);

    @BeforeEach
    void setup() {
        store = new InMemorySessionStore();
        store.put(channel(BigInteger.ZERO, null));
    }

    @Test
    void load_returns_empty_for_unknown() {
        assertThat(store.load("0x" + "ee".repeat(32))).isEmpty();
    }

    @Test
    void put_then_load() {
        assertThat(store.load(CHANNEL)).isPresent();
        assertThat(store.load(CHANNEL).get().deposit()).isEqualTo(BigInteger.valueOf(1000));
    }

    @Test
    void cas_succeeds_when_expected_matches() {
        byte[] sig = new byte[65];
        sig[0] = 0x42;
        boolean ok = store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(100), sig);
        assertThat(ok).isTrue();
        SessionStore.Channel c = store.load(CHANNEL).orElseThrow();
        assertThat(c.lastAccepted()).isEqualTo(100);
        assertThat(c.lastVoucherSignature()).isEqualTo(sig);
    }

    @Test
    void cas_fails_when_expected_mismatches() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(100), new byte[65]);
        boolean ok = store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(200), new byte[65]);
        assertThat(ok).isFalse();
        assertThat(store.load(CHANNEL).get().lastAccepted()).isEqualTo(100);
    }

    @Test
    void update_deposit_preserves_other_fields() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(50), new byte[65]);
        store.updateDeposit(CHANNEL, BigInteger.valueOf(2000));
        SessionStore.Channel c = store.load(CHANNEL).orElseThrow();
        assertThat(c.deposit()).isEqualTo(2000);
        assertThat(c.lastAccepted()).isEqualTo(50);   // preserved
    }

    @Test
    void mark_status_transitions() {
        store.markStatus(CHANNEL, SessionStore.ChannelStatus.CLOSING);
        assertThat(store.load(CHANNEL).get().status()).isEqualTo(SessionStore.ChannelStatus.CLOSING);
        store.markStatus(CHANNEL, SessionStore.ChannelStatus.CLOSED);
        assertThat(store.load(CHANNEL).get().status()).isEqualTo(SessionStore.ChannelStatus.CLOSED);
    }

    @Test
    void concurrent_cas_at_most_one_winner() throws InterruptedException, ExecutionException {
        int threads = 16;
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        try {
            CountDownLatch ready = new CountDownLatch(threads);
            CountDownLatch go = new CountDownLatch(1);
            AtomicInteger winners = new AtomicInteger();

            Future<?>[] futures = new Future<?>[threads];
            for (int i = 0; i < threads; i++) {
                futures[i] = pool.submit(() -> {
                    ready.countDown();
                    go.await();
                    boolean win = store.casLastAccepted(CHANNEL, BigInteger.ZERO,
                        BigInteger.valueOf(System.nanoTime()), new byte[65]);
                    if (win) winners.incrementAndGet();
                    return null;
                });
            }
            ready.await();
            go.countDown();
            for (Future<?> f : futures) f.get();
            assertThat(winners.get()).isEqualTo(1);
        } finally {
            pool.shutdownNow();
        }
    }

    // ── deduct (per-call billing — Rust mpp parity) ──────────────────────────

    @Test
    void deduct_first_call_advances_spent_and_units() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(500), new byte[65]);
        SessionStore.DeductResult r = store.deduct(CHANNEL, BigInteger.valueOf(120));
        assertThat(r.spent()).isEqualTo(120);
        assertThat(r.units()).isEqualTo(1);
        SessionStore.Channel c = store.load(CHANNEL).orElseThrow();
        assertThat(c.spent()).isEqualTo(120);
        assertThat(c.units()).isEqualTo(1);
        // lastAccepted untouched
        assertThat(c.lastAccepted()).isEqualTo(500);
    }

    @Test
    void deduct_multiple_calls_accumulate_until_exhausted() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(500), new byte[65]);
        store.deduct(CHANNEL, BigInteger.valueOf(100));
        store.deduct(CHANNEL, BigInteger.valueOf(300));
        SessionStore.DeductResult r = store.deduct(CHANNEL, BigInteger.valueOf(100));
        assertThat(r.spent()).isEqualTo(500);
        assertThat(r.units()).isEqualTo(3);
    }

    @Test
    void deduct_exceeding_available_throws_insufficient_balance() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(500), new byte[65]);
        store.deduct(CHANNEL, BigInteger.valueOf(400));
        assertThatThrownBy(() -> store.deduct(CHANNEL, BigInteger.valueOf(101)))
            .isInstanceOf(InsufficientBalanceError.class)
            .hasMessageContaining("requested 101")
            .hasMessageContaining("available 100");
    }

    @Test
    void deduct_rejects_zero_or_negative_amount() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(500), new byte[65]);
        assertThatThrownBy(() -> store.deduct(CHANNEL, BigInteger.ZERO))
            .isInstanceOf(IllegalArgumentException.class);
        assertThatThrownBy(() -> store.deduct(CHANNEL, BigInteger.valueOf(-5)))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void deduct_unknown_channel_throws_not_found() {
        assertThatThrownBy(() -> store.deduct("0x" + "ff".repeat(32), BigInteger.valueOf(1)))
            .isInstanceOf(ChannelNotFoundError.class);
    }

    @Test
    void deduct_preserves_other_channel_fields() {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(500), new byte[65]);
        store.updateSettledOnChain(CHANNEL, BigInteger.valueOf(200));
        store.deduct(CHANNEL, BigInteger.valueOf(50));
        SessionStore.Channel c = store.load(CHANNEL).orElseThrow();
        assertThat(c.deposit()).isEqualTo(1000);            // preserved
        assertThat(c.lastAccepted()).isEqualTo(500);        // preserved
        assertThat(c.settledOnChain()).isEqualTo(200);      // preserved
        assertThat(c.status()).isEqualTo(SessionStore.ChannelStatus.OPEN);
    }

    @Test
    void deduct_concurrent_callers_serialize_correctly() throws InterruptedException, ExecutionException {
        store.casLastAccepted(CHANNEL, BigInteger.ZERO, BigInteger.valueOf(10_000), new byte[65]);
        int threads = 16;
        int perThread = 50;
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        try {
            CountDownLatch ready = new CountDownLatch(threads);
            CountDownLatch go = new CountDownLatch(1);
            Future<?>[] futures = new Future<?>[threads];
            for (int i = 0; i < threads; i++) {
                futures[i] = pool.submit(() -> {
                    ready.countDown();
                    go.await();
                    for (int n = 0; n < perThread; n++) {
                        store.deduct(CHANNEL, BigInteger.ONE);
                    }
                    return null;
                });
            }
            ready.await();
            go.countDown();
            for (Future<?> f : futures) f.get();
            SessionStore.Channel c = store.load(CHANNEL).orElseThrow();
            assertThat(c.spent()).isEqualTo(BigInteger.valueOf(threads * perThread));
            assertThat(c.units()).isEqualTo(threads * perThread);
        } finally {
            pool.shutdownNow();
        }
    }

    private SessionStore.Channel channel(BigInteger lastAccepted, byte[] sig) {
        return new SessionStore.Channel(
            CHANNEL,
            "0xpayer000000000000000000000000000000000aa",
            "0xpayee000000000000000000000000000000000bb",
            "0xtoken000000000000000000000000000000000cc",
            "0xescrow0000000000000000000000000000000dd",
            196L,
            "0x0000000000000000000000000000000000000000",
            BigInteger.valueOf(1000), lastAccepted, sig,
            BigInteger.ZERO, SessionStore.ChannelStatus.OPEN);
    }
}
