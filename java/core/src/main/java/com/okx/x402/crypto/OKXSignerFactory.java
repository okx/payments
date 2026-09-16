// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

/**
 * Factory for creating OKX EVM signers.
 */
public final class OKXSignerFactory {

    private OKXSignerFactory() {
    }

    /**
     * Configuration for creating an OKX signer.
     */
    public static class OKXSignerConfig {
        private String privateKey;

        /**
         * Sets the private key for signing.
         *
         * @param privateKey 0x-prefixed hex private key
         * @return this config
         */
        public OKXSignerConfig privateKey(String privateKey) {
            this.privateKey = privateKey;
            return this;
        }

        /**
         * Gets the configured private key.
         *
         * @return the private key
         */
        public String getPrivateKey() {
            return privateKey;
        }
    }

    /**
     * Creates an EVM EIP-3009 signer.
     * Throws immediately if no privateKey provided.
     *
     * @param config signer configuration
     * @return configured OKXEvmSigner
     * @throws IllegalArgumentException if config is null or privateKey is missing
     */
    public static OKXEvmSigner createOKXSigner(OKXSignerConfig config) {
        if (config == null || config.getPrivateKey() == null
                || config.getPrivateKey().isEmpty()) {
            throw new IllegalArgumentException("privateKey is required");
        }
        return new OKXEvmSigner(config.getPrivateKey());
    }
}
