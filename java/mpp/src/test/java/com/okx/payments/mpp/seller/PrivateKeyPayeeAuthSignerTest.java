package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.voucher.Eip712Hashing;
import com.okx.payments.mpp.voucher.Eip712Verifier;
import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Keys;

import java.math.BigInteger;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class PrivateKeyPayeeAuthSignerTest {

    private static final BigInteger PRIV = BigInteger.valueOf(0xABCDEFL);
    private static final String EXPECTED_ADDR = "0x" + Keys.getAddress(ECKeyPair.create(PRIV));
    private static final String CHANNEL_ID =
        "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f";

    private final EvmPaymentChannelDomain domain = EvmPaymentChannelDomain.defaults();

    @Test
    void from_hex_factory_derives_correct_address() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromHex(PRIV.toString(16));
        assertThat(s.address()).isEqualToIgnoringCase(EXPECTED_ADDR);
    }

    @Test
    void from_bytes_factory_derives_correct_address() {
        byte[] padded = paddedKey();
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromBytes(padded);
        assertThat(s.address()).isEqualToIgnoringCase(EXPECTED_ADDR);
    }

    @Test
    void from_env_var_reads_via_function() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromEnvVar(
            "TEST_KEY", Map.of("TEST_KEY", PRIV.toString(16))::get);
        assertThat(s.address()).isEqualToIgnoringCase(EXPECTED_ADDR);
    }

    @Test
    void from_env_var_missing_throws() {
        assertThatThrownBy(() -> PrivateKeyPayeeAuthSigner.fromEnvVar("X", k -> null))
            .isInstanceOf(IllegalStateException.class)
            .hasMessageContaining("X");
    }

    @Test
    void verify_address_passes_when_match() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromHex(PRIV.toString(16))
            .verifyAddress(EXPECTED_ADDR);
        assertThat(s.address()).isEqualToIgnoringCase(EXPECTED_ADDR);
    }

    @Test
    void verify_address_throws_on_mismatch() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromHex(PRIV.toString(16));
        assertThatThrownBy(() -> s.verifyAddress("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"))
            .isInstanceOf(IllegalStateException.class)
            .hasMessageContaining("derives address")
            .hasMessageContaining("config says");
    }

    @Test
    void verify_address_null_passes_through() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromHex(PRIV.toString(16))
            .verifyAddress(null);
        assertThat(s.address()).isEqualToIgnoringCase(EXPECTED_ADDR);
    }

    @Test
    void sign_settle_authorization_recovers_to_payee_address() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromBytes(paddedKey());
        BigInteger cum = BigInteger.valueOf(250000);
        BigInteger nonce = BigInteger.valueOf(42);
        BigInteger deadline = BigInteger.valueOf(1745500000L);

        byte[] sig = s.signSettleAuthorization(domain,
            new PayeeAuthSigner.AuthData(CHANNEL_ID, cum, nonce, deadline));
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.settleAuthStructHash(CHANNEL_ID, cum, nonce, deadline));
        Eip712Verifier.verify(digest, sig, s.address());
    }

    @Test
    void sign_close_authorization_recovers_to_payee_address() {
        PrivateKeyPayeeAuthSigner s = PrivateKeyPayeeAuthSigner.fromBytes(paddedKey());
        BigInteger cum = BigInteger.valueOf(500000);
        BigInteger nonce = BigInteger.valueOf(99);
        BigInteger deadline = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);   // uint256 max

        byte[] sig = s.signCloseAuthorization(domain,
            new PayeeAuthSigner.AuthData(CHANNEL_ID, cum, nonce, deadline));
        byte[] digest = Eip712Hashing.digest(
            Eip712Hashing.domainSeparator(domain),
            Eip712Hashing.closeAuthStructHash(CHANNEL_ID, cum, nonce, deadline));
        Eip712Verifier.verify(digest, sig, s.address());
    }

    private static byte[] paddedKey() {
        byte[] raw = PRIV.toByteArray();
        byte[] padded = new byte[32];
        int srcOffset = raw.length > 32 ? raw.length - 32 : 0;
        int srcLen = Math.min(raw.length, 32);
        System.arraycopy(raw, srcOffset, padded, 32 - srcLen, srcLen);
        return padded;
    }
}
