package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Signer;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.math.BigInteger;
import java.util.Objects;

/**
 * Default {@link PayeeAuthSigner} backed by an in-memory secp256k1 private key.
 *
 * <p>Three factories cover the env-var and self-held-key tiers:
 * <ul>
 *   <li>{@link #fromEnvVar(String)} — read hex private key from env var (dev / single-tenant)</li>
 *   <li>{@link #fromHex(String)} — caller-supplied hex string</li>
 *   <li>{@link #fromBytes(byte[])} — caller-supplied 32-byte raw key</li>
 * </ul>
 *
 * <p>For tier ③ (KMS / Ledger / TEE / HSM), implement {@link PayeeAuthSigner} directly.
 */
public final class PrivateKeyPayeeAuthSigner implements PayeeAuthSigner {

    private final ECKeyPair keyPair;
    private final String address;

    private PrivateKeyPayeeAuthSigner(ECKeyPair keyPair) {
        this.keyPair = Objects.requireNonNull(keyPair);
        this.address = "0x" + Keys.getAddress(keyPair);
    }

    public static PrivateKeyPayeeAuthSigner fromHex(String hexKey) {
        if (hexKey == null) throw new IllegalArgumentException("hexKey null");
        String trimmed = hexKey.startsWith("0x") || hexKey.startsWith("0X")
            ? hexKey.substring(2) : hexKey;
        return new PrivateKeyPayeeAuthSigner(ECKeyPair.create(new BigInteger(trimmed, 16)));
    }

    public static PrivateKeyPayeeAuthSigner fromBytes(byte[] privKey32) {
        if (privKey32 == null || privKey32.length != 32) {
            throw new IllegalArgumentException("private key must be 32 bytes");
        }
        return new PrivateKeyPayeeAuthSigner(ECKeyPair.create(new BigInteger(1, privKey32)));
    }

    public static PrivateKeyPayeeAuthSigner fromEnvVar(String envName) {
        return fromEnvVar(envName, System::getenv);
    }

    public static PrivateKeyPayeeAuthSigner fromEnvVar(String envName,
                                                       java.util.function.Function<String, String> env) {
        String v = env.apply(envName);
        if (v == null || v.isBlank()) {
            throw new IllegalStateException("env var " + envName + " not set");
        }
        return fromHex(v);
    }

    /** Self-check the derived address against caller expectation; throws if mismatch. */
    public PrivateKeyPayeeAuthSigner verifyAddress(String expected) {
        if (expected == null) return this;
        String norm = expected.startsWith("0x") ? expected : "0x" + expected;
        if (!address.equalsIgnoreCase(norm)) {
            throw new IllegalStateException(
                "private key derives address " + address + " but config says " + norm
                    + " — refusing to start with mismatched payee key");
        }
        return this;
    }

    @Override
    public byte[] signSettleAuthorization(EvmPaymentChannelDomain domain, AuthData data) {
        byte[] structHash = Eip712Hashing.settleAuthStructHash(
            data.channelId(), data.cumulativeAmount(), data.nonce(), data.deadline());
        byte[] digest = Eip712Hashing.digest(Eip712Hashing.domainSeparator(domain), structHash);
        return Eip712Signer.sign(digest, keyPair);
    }

    @Override
    public byte[] signCloseAuthorization(EvmPaymentChannelDomain domain, AuthData data) {
        byte[] structHash = Eip712Hashing.closeAuthStructHash(
            data.channelId(), data.cumulativeAmount(), data.nonce(), data.deadline());
        byte[] digest = Eip712Hashing.digest(Eip712Hashing.domainSeparator(domain), structHash);
        return Eip712Signer.sign(digest, keyPair);
    }

    @Override
    public String address() {
        return address;
    }
}
