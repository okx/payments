// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.config;

import org.junit.jupiter.api.Test;

import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

import static org.junit.jupiter.api.Assertions.*;

class AssetRegistryTest {

    @Test
    void xlayerUsdtResolvesAutomatically() {
        ResolvedPrice resolved = AssetRegistry.resolvePrice("$0.01", "eip155:196");
        assertNotNull(resolved);
        assertEquals("10000", resolved.amount());
        assertNotNull(resolved.asset());
        assertNotNull(resolved.extra());
    }

    @Test
    void eip712NameContainsUnicode() {
        AssetConfig usdt = AssetRegistry.get("eip155:196", "USDT");
        assertNotNull(usdt);
        // \u20AE is U+20AE
        assertTrue(usdt.getEip712Name().contains("\u20AE"),
                "EIP-712 name should contain Unicode \u20AE (U+20AE)");
        assertEquals("USD\u20AE0", usdt.getEip712Name());
    }

    @Test
    void testnetNotPreRegistered() {
        // X Layer testnet (eip155:195) must not ship with a placeholder asset
        // — signing against a fake contract address silently produces a
        // corrupt EIP-712 domain. Callers that need testnet register their
        // own real asset via AssetRegistry.register(...).
        assertNull(AssetRegistry.getDefault("eip155:195"),
                "X Layer testnet must stay unregistered by default");
        assertThrows(IllegalArgumentException.class,
                () -> AssetRegistry.resolvePrice("$1.00", "eip155:195"));
    }

    @Test
    void contractAddressAndDecimalsCorrect() {
        AssetConfig usdt = AssetRegistry.get("eip155:196", "USDT");
        assertNotNull(usdt);
        assertEquals("0x779ded0c9e1022225f8e0630b35a9b54be713736", usdt.getContractAddress());
        assertEquals(6, usdt.getDecimals());
    }

    @Test
    void unknownNetworkThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> AssetRegistry.resolvePrice("$1.00", "eip155:999999"));
    }

    @Test
    void priceResolutionCorrect() {
        ResolvedPrice resolved = AssetRegistry.resolvePrice("$1.00", "eip155:196");
        assertEquals("1000000", resolved.amount());
    }

    @Test
    void usdgRegisteredForXLayer() {
        AssetConfig usdg = AssetRegistry.get("eip155:196", "USDG");
        assertNotNull(usdg, "USDG should be registered for eip155:196");
        assertEquals("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8",
                usdg.getContractAddress());
        assertEquals(6, usdg.getDecimals());
        assertEquals("USDG", usdg.getEip712Name());
        assertEquals("2", usdg.getEip712Version());
    }

    @Test
    void customAssetRegistration() {
        AssetRegistry.register("eip155:999", AssetConfig.builder()
                .symbol("TEST")
                .contractAddress("0xtest")
                .decimals(18)
                .eip712Name("Test")
                .eip712Version("1")
                .transferMethod("eip3009")
                .build());

        AssetConfig custom = AssetRegistry.get("eip155:999", "TEST");
        assertNotNull(custom);
        assertEquals("TEST", custom.getSymbol());
    }

    /**
     * Concurrency smoke test: many threads register to the same network at
     * once. Before the registry switched its inner list to
     * {@link java.util.concurrent.CopyOnWriteArrayList}, ArrayList.add() races
     * could drop elements or throw ArrayIndexOutOfBounds. Pin the fix so a
     * regression to a non-thread-safe collection trips the test.
     *
     * @throws InterruptedException if executor shutdown is interrupted
     */
    @Test
    void concurrentRegistrationDoesNotDropEntries() throws InterruptedException {
        final String network = "eip155:910001";   // unique per test run
        final int threads = 16;
        final int perThread = 50;

        ExecutorService pool = Executors.newFixedThreadPool(threads);
        CountDownLatch start = new CountDownLatch(1);
        CountDownLatch done = new CountDownLatch(threads);
        AtomicInteger failures = new AtomicInteger();

        for (int t = 0; t < threads; t++) {
            final int tid = t;
            pool.submit(() -> {
                try {
                    start.await();
                    for (int i = 0; i < perThread; i++) {
                        AssetRegistry.register(network, AssetConfig.builder()
                                .symbol("T" + tid + "_" + i)
                                .contractAddress("0x" + tid + "_" + i)
                                .decimals(6)
                                .eip712Name("Test")
                                .eip712Version("1")
                                .transferMethod("eip3009")
                                .build());
                    }
                } catch (Throwable e) {
                    failures.incrementAndGet();
                } finally {
                    done.countDown();
                }
            });
        }

        start.countDown();
        assertTrue(done.await(30, TimeUnit.SECONDS),
                "concurrent registers must finish within 30s");
        pool.shutdownNow();

        assertEquals(0, failures.get(),
                "no thread should observe an exception during register()");

        // Spot-check: every (tid, i) tuple must be retrievable. If
        // ArrayList.add() lost writes due to internal-state corruption the
        // get() lookups below would return null.
        for (int tid = 0; tid < threads; tid++) {
            for (int i = 0; i < perThread; i++) {
                AssetConfig found = AssetRegistry.get(network, "T" + tid + "_" + i);
                assertNotNull(found,
                        "registration lost: tid=" + tid + " i=" + i);
            }
        }
    }
}
