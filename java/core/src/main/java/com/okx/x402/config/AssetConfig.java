// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.config;

import java.math.BigDecimal;
import java.math.RoundingMode;

/**
 * Configuration for a token asset on a specific chain.
 * Contains EIP-712 domain info needed for signing.
 */
public class AssetConfig {

    private final String symbol;
    private final String contractAddress;
    private final int decimals;
    private final String eip712Name;
    private final String eip712Version;
    private final String transferMethod;

    private AssetConfig(Builder builder) {
        this.symbol = builder.symbol;
        this.contractAddress = builder.contractAddress;
        this.decimals = builder.decimals;
        this.eip712Name = builder.eip712Name;
        this.eip712Version = builder.eip712Version;
        this.transferMethod = builder.transferMethod;
    }

    /** Returns a new builder for AssetConfig. */
    public static Builder builder() {
        return new Builder();
    }

    /** Returns the token symbol (e.g. "USDT"). */
    public String getSymbol() {
        return symbol;
    }

    /** Returns the token contract address. */
    public String getContractAddress() {
        return contractAddress;
    }

    /** Returns the token decimals (e.g. 6). */
    public int getDecimals() {
        return decimals;
    }

    /** Returns the EIP-712 domain name (e.g. "USD₮0"). */
    public String getEip712Name() {
        return eip712Name;
    }

    /** Returns the EIP-712 domain version. */
    public String getEip712Version() {
        return eip712Version;
    }

    /** Returns the transfer method (e.g. "eip3009"). */
    public String getTransferMethod() {
        return transferMethod;
    }

    /**
     * Convert human-readable amount to atomic units string.
     *
     * @param humanAmount amount in human-readable form (e.g. 0.01)
     * @return atomic units as string (e.g. "10000" for 0.01 with 6 decimals)
     */
    public String toAtomicUnits(BigDecimal humanAmount) {
        return humanAmount.movePointRight(decimals)
                .setScale(0, RoundingMode.DOWN)
                .toBigInteger().toString();
    }

    /**
     * Convert USD price string to atomic units (assumes 1:1 stablecoin).
     *
     * @param usdPrice price string like "$0.01"
     * @return atomic units as string
     */
    public String fromUsdPrice(String usdPrice) {
        String cleaned = usdPrice.replace("$", "").trim();
        return toAtomicUnits(new BigDecimal(cleaned));
    }

    /** Builder for AssetConfig. */
    public static class Builder {
        private String symbol;
        private String contractAddress;
        private int decimals;
        private String eip712Name;
        private String eip712Version;
        private String transferMethod;

        /** Sets the token symbol. */
        public Builder symbol(String symbol) {
            this.symbol = symbol;
            return this;
        }

        /** Sets the contract address. */
        public Builder contractAddress(String contractAddress) {
            this.contractAddress = contractAddress;
            return this;
        }

        /** Sets the token decimals. */
        public Builder decimals(int decimals) {
            this.decimals = decimals;
            return this;
        }

        /** Sets the EIP-712 domain name. */
        public Builder eip712Name(String eip712Name) {
            this.eip712Name = eip712Name;
            return this;
        }

        /** Sets the EIP-712 domain version. */
        public Builder eip712Version(String eip712Version) {
            this.eip712Version = eip712Version;
            return this;
        }

        /** Sets the transfer method. */
        public Builder transferMethod(String transferMethod) {
            this.transferMethod = transferMethod;
            return this;
        }

        /** Builds the AssetConfig instance. */
        public AssetConfig build() {
            return new AssetConfig(this);
        }
    }
}
