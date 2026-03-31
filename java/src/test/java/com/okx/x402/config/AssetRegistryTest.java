package com.okx.x402.config;

import org.junit.jupiter.api.Test;

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
    void testnetRegistered() {
        AssetConfig testnet = AssetRegistry.getDefault("eip155:195");
        assertNotNull(testnet, "X Layer testnet should be registered");
        assertEquals("USDT", testnet.getSymbol());
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
}
