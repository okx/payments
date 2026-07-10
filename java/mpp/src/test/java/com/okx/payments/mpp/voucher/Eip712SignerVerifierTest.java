package com.okx.payments.mpp.voucher;

import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;
import org.web3j.utils.Numeric;

import java.math.BigInteger;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class Eip712SignerVerifierTest {

    private static final String CHANNEL_ID =
        "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f";

    /** Deterministic test key (private key = 1). */
    private final ECKeyPair key = ECKeyPair.create(BigInteger.ONE);
    private final String signerAddr = "0x" + Keys.getAddress(key);

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();

    @Test
    void sign_voucher_then_verify_recovers_signer() {
        byte[] digest = voucherDigest(BigInteger.valueOf(250000));
        byte[] sig = Eip712Signer.sign(digest, key);
        assertThat(sig).hasSize(65);
        // Verify must not throw
        Eip712Verifier.verify(digest, sig, signerAddr);
    }

    @Test
    void verify_rejects_64_byte_eip2098_compact() {
        byte[] digest = voucherDigest(BigInteger.valueOf(100));
        byte[] sig65 = Eip712Signer.sign(digest, key);
        byte[] compact = new byte[64];
        System.arraycopy(sig65, 0, compact, 0, 64);

        assertThatThrownBy(() -> Eip712Verifier.verify(digest, compact, signerAddr))
            .isInstanceOf(Eip712Verifier.Eip712VerifyException.class)
            .hasMessageContaining("65 bytes");
    }

    @Test
    void verify_rejects_high_s() {
        byte[] digest = voucherDigest(BigInteger.valueOf(100));
        byte[] sig = Eip712Signer.sign(digest, key);
        // Compute high-s = N - s
        BigInteger n = new BigInteger("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141", 16);
        byte[] sBytes = new byte[32];
        System.arraycopy(sig, 32, sBytes, 0, 32);
        BigInteger s = new BigInteger(1, sBytes);
        BigInteger highS = n.subtract(s);

        byte[] tampered = sig.clone();
        byte[] highSBytes = Numeric.toBytesPadded(highS, 32);
        System.arraycopy(highSBytes, 0, tampered, 32, 32);
        // Flip v as well so signature is "valid" cryptographically but high-s — verifier should still reject.
        tampered[64] = (byte) (sig[64] == 27 ? 28 : 27);

        assertThatThrownBy(() -> Eip712Verifier.verify(digest, tampered, signerAddr))
            .isInstanceOf(Eip712Verifier.Eip712VerifyException.class)
            .hasMessageContaining("high-s");
    }

    @Test
    void verify_rejects_invalid_v() {
        byte[] digest = voucherDigest(BigInteger.valueOf(100));
        byte[] sig = Eip712Signer.sign(digest, key);
        sig[64] = 0;
        assertThatThrownBy(() -> Eip712Verifier.verify(digest, sig, signerAddr))
            .isInstanceOf(Eip712Verifier.Eip712VerifyException.class)
            .hasMessageContaining("v must be");
    }

    @Test
    void verify_rejects_signer_mismatch() {
        byte[] digest = voucherDigest(BigInteger.valueOf(100));
        byte[] sig = Eip712Signer.sign(digest, key);
        String otherAddr = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        assertThatThrownBy(() -> Eip712Verifier.verify(digest, sig, otherAddr))
            .isInstanceOf(Eip712Verifier.Eip712VerifyException.class)
            .hasMessageContaining("recovered signer");
    }

    @Test
    void recover_returns_lowercased_address() {
        byte[] digest = voucherDigest(BigInteger.valueOf(100));
        byte[] sig = Eip712Signer.sign(digest, key);
        String recovered = Eip712Verifier.recover(digest, sig);
        assertThat(recovered).isEqualToIgnoringCase(signerAddr);
    }

    @Test
    void settle_authorization_signing_roundtrip() {
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.settleAuthStructHash(
                CHANNEL_ID,
                BigInteger.valueOf(250000),
                BigInteger.valueOf(42),
                BigInteger.valueOf(1745500000L)));
        byte[] sig = Eip712Signer.sign(digest, key);
        Eip712Verifier.verify(digest, sig, signerAddr);
    }

    @Test
    void close_authorization_signing_roundtrip() {
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.closeAuthStructHash(
                CHANNEL_ID,
                BigInteger.valueOf(500000),
                BigInteger.valueOf(99),
                BigInteger.valueOf(1745500600L)));
        byte[] sig = Eip712Signer.sign(digest, key);
        Eip712Verifier.verify(digest, sig, signerAddr);
    }

    private byte[] voucherDigest(BigInteger cumulativeAmount) {
        return Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.voucherStructHash(CHANNEL_ID, cumulativeAmount));
    }
}
