// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.config;

import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;

/**
 * Registry of supported token assets per chain.
 * Pre-configured with X Layer USDT; extensible via register().
 */
public final class AssetRegistry {

    private static final Map<String, List<AssetConfig>> NETWORK_ASSETS = new ConcurrentHashMap<>();

    static {
        // X Layer Mainnet USDT
        register("eip155:196", AssetConfig.builder()
                .symbol("USDT")
                .contractAddress("0x779ded0c9e1022225f8e0630b35a9b54be713736")
                .decimals(6)
                .eip712Name("USD\u20AE0")   // "USD₮0" — Unicode U+20AE
                .eip712Version("1")
                .transferMethod("eip3009")
                .build());

        register("eip155:196", AssetConfig.builder()
                .symbol("USDG")
                .contractAddress("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")
                .decimals(6)
                .eip712Name("USDG")
                .eip712Version("2")
                .transferMethod("eip3009")
                .build());
    }

    private AssetRegistry() {
    }

    /**
     * Get the default asset for a network (first registered, typically USDT).
     *
     * @param network CAIP-2 network identifier (e.g. "eip155:196")
     * @return default asset config, or null if none registered
     */
    public static AssetConfig getDefault(String network) {
        List<AssetConfig> assets = NETWORK_ASSETS.get(network);
        if (assets == null || assets.isEmpty()) {
            return null;
        }
        return assets.get(0);
    }

    /**
     * Get a specific asset by network and symbol.
     *
     * @param network CAIP-2 network identifier
     * @param symbol token symbol (e.g. "USDT")
     * @return matching asset config, or null if not found
     */
    public static AssetConfig get(String network, String symbol) {
        List<AssetConfig> assets = NETWORK_ASSETS.get(network);
        if (assets == null) {
            return null;
        }
        return assets.stream()
                .filter(a -> a.getSymbol().equals(symbol))
                .findFirst()
                .orElse(null);
    }

    /**
     * Get a specific asset by network and contract address (case-insensitive).
     *
     * @param network CAIP-2 network identifier
     * @param contractAddress token contract address (e.g. "0x779d...")
     * @return matching asset config, or null if not found
     */
    public static AssetConfig getByAddress(String network, String contractAddress) {
        List<AssetConfig> assets = NETWORK_ASSETS.get(network);
        if (assets == null || contractAddress == null) {
            return null;
        }
        return assets.stream()
                .filter(a -> a.getContractAddress().equalsIgnoreCase(contractAddress))
                .findFirst()
                .orElse(null);
    }

    /**
     * Resolve a USD price string for a given network using the default asset.
     *
     * @param price USD price string (e.g. "$0.01") or atomic units string
     * @param network CAIP-2 network identifier
     * @return resolved price with atomic amount, asset address, and extra fields
     * @throws IllegalArgumentException if no default asset configured for network
     */
    public static ResolvedPrice resolvePrice(String price, String network) {
        return resolvePrice(price, network, null);
    }

    /**
     * Resolve a USD price string for a specific asset on a given network.
     * When {@code assetAddress} is null, falls back to the network default.
     *
     * @param price USD price string (e.g. "$0.01") or atomic units string
     * @param network CAIP-2 network identifier
     * @param assetAddress contract address of the asset to charge in, or null
     *                     to use the registry default for the network
     * @return resolved price with atomic amount, asset address, and extra fields
     * @throws IllegalArgumentException if no matching asset is registered
     */
    public static ResolvedPrice resolvePrice(String price, String network, String assetAddress) {
        AssetConfig asset = assetAddress != null
                ? getByAddress(network, assetAddress)
                : getDefault(network);
        if (asset == null) {
            throw new IllegalArgumentException(
                    assetAddress != null
                            ? "No asset registered at " + assetAddress + " on " + network
                                    + " — call AssetRegistry.register(...) first"
                            : "No default asset configured for network " + network);
        }
        return new ResolvedPrice(
                asset.fromUsdPrice(price),
                asset.getContractAddress(),
                Map.of("name", asset.getEip712Name(),
                        "version", asset.getEip712Version(),
                        "transferMethod", asset.getTransferMethod(),
                        "symbol", asset.getSymbol())
        );
    }

    /**
     * Register a custom asset for a network.
     *
     * @param network CAIP-2 network identifier
     * @param config asset configuration to register
     */
    public static void register(String network, AssetConfig config) {
        // CopyOnWriteArrayList: read-mostly access pattern (lookups on every
        // request, registrations only at boot / dynamic asset add). Lets us
        // drop external locking on both register() and the read paths.
        NETWORK_ASSETS.computeIfAbsent(network, k -> new CopyOnWriteArrayList<>())
                .add(config);
    }
}
