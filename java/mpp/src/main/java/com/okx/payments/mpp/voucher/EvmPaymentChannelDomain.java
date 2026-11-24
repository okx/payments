package com.okx.payments.mpp.voucher;

import org.web3j.utils.Numeric;

import java.util.Objects;

/**
 * EIP-712 Domain for the {@code EvmPaymentChannel} escrow contract.
 *
 * <p>Defaults match smart-account {@code MppVoucherVerifyService.java:55-57}:
 * {@code name="EVM Payment Channel"}, {@code version="1"},
 * {@code chainId=196}, {@code verifyingContract=0x5E55...}.
 *
 * <p>Override priority (high → low): builder → env var → default constant.
 */
public final class EvmPaymentChannelDomain {

    public static final String DEFAULT_NAME = "EVM Payment Channel";
    public static final String DEFAULT_VERSION = "1";
    public static final long DEFAULT_CHAIN_ID = 196L;
    public static final String DEFAULT_ESCROW_ADDRESS = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b";

    public static final String ENV_NAME = "MPP_DOMAIN_NAME";
    public static final String ENV_VERSION = "MPP_DOMAIN_VERSION";
    public static final String ENV_CHAIN_ID = "MPP_CHAIN_ID";
    public static final String ENV_ESCROW_ADDRESS = "MPP_ESCROW_ADDRESS";

    private final String name;
    private final String version;
    private final long chainId;
    private final String escrowAddress;

    private EvmPaymentChannelDomain(String name, String version, long chainId, String escrowAddress) {
        this.name = Objects.requireNonNull(name, "name");
        this.version = Objects.requireNonNull(version, "version");
        this.chainId = chainId;
        this.escrowAddress = requireAddress(escrowAddress);
    }

    public String name() { return name; }
    public String version() { return version; }
    public long chainId() { return chainId; }
    public String escrowAddress() { return escrowAddress; }

    /** Pure defaults (ignores env vars). */
    public static EvmPaymentChannelDomain defaults() {
        return new EvmPaymentChannelDomain(DEFAULT_NAME, DEFAULT_VERSION, DEFAULT_CHAIN_ID, DEFAULT_ESCROW_ADDRESS);
    }

    /** Defaults overridden by env vars (when set), else defaults. */
    public static EvmPaymentChannelDomain fromEnv() {
        return fromEnv(System::getenv);
    }

    /** Test-friendly: pass a function that resolves env vars. */
    public static EvmPaymentChannelDomain fromEnv(java.util.function.Function<String, String> env) {
        String n = orDefault(env.apply(ENV_NAME), DEFAULT_NAME);
        String v = orDefault(env.apply(ENV_VERSION), DEFAULT_VERSION);
        String chainStr = env.apply(ENV_CHAIN_ID);
        long c = chainStr != null && !chainStr.isBlank() ? Long.parseLong(chainStr.trim()) : DEFAULT_CHAIN_ID;
        String e = orDefault(env.apply(ENV_ESCROW_ADDRESS), DEFAULT_ESCROW_ADDRESS);
        return new EvmPaymentChannelDomain(n, v, c, e);
    }

    public static Builder builder() {
        return new Builder();
    }

    public Builder toBuilder() {
        return new Builder()
            .name(name).version(version).chainId(chainId).escrowAddress(escrowAddress);
    }

    public static final class Builder {
        private String name = DEFAULT_NAME;
        private String version = DEFAULT_VERSION;
        private long chainId = DEFAULT_CHAIN_ID;
        private String escrowAddress = DEFAULT_ESCROW_ADDRESS;

        public Builder name(String v) { this.name = v; return this; }
        public Builder version(String v) { this.version = v; return this; }
        public Builder chainId(long v) { this.chainId = v; return this; }
        public Builder escrowAddress(String v) { this.escrowAddress = v; return this; }

        public EvmPaymentChannelDomain build() {
            return new EvmPaymentChannelDomain(name, version, chainId, escrowAddress);
        }
    }

    private static String orDefault(String v, String d) {
        return v == null || v.isBlank() ? d : v;
    }

    private static String requireAddress(String s) {
        if (s == null || !s.matches("^0x[0-9a-fA-F]{40}$")) {
            throw new IllegalArgumentException("escrowAddress must be 0x + 40 hex chars: " + s);
        }
        return Numeric.prependHexPrefix(s.substring(2).toLowerCase());
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (!(o instanceof EvmPaymentChannelDomain d)) return false;
        return chainId == d.chainId
            && name.equals(d.name)
            && version.equals(d.version)
            && escrowAddress.equalsIgnoreCase(d.escrowAddress);
    }

    @Override
    public int hashCode() {
        return Objects.hash(name, version, chainId, escrowAddress.toLowerCase());
    }

    @Override
    public String toString() {
        return "EvmPaymentChannelDomain{name=" + name + ", version=" + version
            + ", chainId=" + chainId + ", escrow=" + escrowAddress + "}";
    }
}
