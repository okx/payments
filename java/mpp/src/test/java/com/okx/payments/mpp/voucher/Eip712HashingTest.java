package com.okx.payments.mpp.voucher;

import org.junit.jupiter.api.Test;
import org.web3j.utils.Numeric;

import java.math.BigInteger;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class Eip712HashingTest {

    private static final String CHANNEL_ID =
        "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f";
    private static final String ESCROW = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b";

    @Test
    void typehashes_are_stable() {
        // Pin the typehash bytes — any spec rewording of the EIP-712 type strings would change these.
        assertThat(Numeric.toHexString(Eip712Hashing.EIP712_DOMAIN_TYPEHASH))
            .isEqualTo("0x8b73c3c69bb8fe3d512ecc4cf759cc79239f7b179b0ffacaa9a75d522b39400f");
    }

    @Test
    void domain_separator_with_defaults_is_deterministic() {
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.defaults();
        byte[] sep1 = Eip712Hashing.domainSeparator(d);
        byte[] sep2 = Eip712Hashing.domainSeparator(d);
        assertThat(sep1).isEqualTo(sep2);
        assertThat(sep1).hasSize(32);
    }

    @Test
    void domain_separator_changes_when_chain_id_changes() {
        EvmPaymentChannelDomain d1 = EvmPaymentChannelDomain.builder().chainId(196).build();
        EvmPaymentChannelDomain d2 = EvmPaymentChannelDomain.builder().chainId(1).build();
        assertThat(Eip712Hashing.domainSeparator(d1))
            .isNotEqualTo(Eip712Hashing.domainSeparator(d2));
    }

    @Test
    void voucher_struct_hash_is_deterministic() {
        byte[] h1 = Eip712Hashing.voucherStructHash(CHANNEL_ID, BigInteger.valueOf(250000));
        byte[] h2 = Eip712Hashing.voucherStructHash(CHANNEL_ID, BigInteger.valueOf(250000));
        assertThat(h1).isEqualTo(h2);
        assertThat(h1).hasSize(32);
    }

    @Test
    void voucher_struct_hash_differs_for_different_amounts() {
        byte[] h1 = Eip712Hashing.voucherStructHash(CHANNEL_ID, BigInteger.valueOf(100));
        byte[] h2 = Eip712Hashing.voucherStructHash(CHANNEL_ID, BigInteger.valueOf(200));
        assertThat(h1).isNotEqualTo(h2);
    }

    @Test
    void uint256_encoding_pads_left_zero() {
        byte[] one = Eip712Hashing.uint256(BigInteger.ONE);
        assertThat(one).hasSize(32);
        assertThat(one[31]).isEqualTo((byte) 0x01);
        for (int i = 0; i < 31; i++) {
            assertThat(one[i]).isEqualTo((byte) 0x00);
        }
    }

    @Test
    void uint256_handles_uint256_max() {
        BigInteger max = BigInteger.ONE.shiftLeft(256).subtract(BigInteger.ONE);
        byte[] bytes = Eip712Hashing.uint256(max);
        assertThat(bytes).hasSize(32);
        for (int i = 0; i < 32; i++) {
            assertThat(bytes[i]).isEqualTo((byte) 0xff);
        }
    }

    @Test
    void uint256_rejects_overflow() {
        BigInteger overflow = BigInteger.ONE.shiftLeft(256);
        assertThatThrownBy(() -> Eip712Hashing.uint256(overflow))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void uint256_rejects_negative() {
        assertThatThrownBy(() -> Eip712Hashing.uint256(BigInteger.valueOf(-1)))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void address_encoding_left_pads_to_32_bytes() {
        byte[] enc = Eip712Hashing.address(ESCROW);
        assertThat(enc).hasSize(32);
        // first 12 bytes zero
        for (int i = 0; i < 12; i++) {
            assertThat(enc[i]).isEqualTo((byte) 0x00);
        }
    }

    @Test
    void settle_auth_struct_hash_uses_all_fields() {
        byte[] h1 = Eip712Hashing.settleAuthStructHash(
            CHANNEL_ID, BigInteger.valueOf(100), BigInteger.valueOf(1), BigInteger.valueOf(1745500000L));
        byte[] h2 = Eip712Hashing.settleAuthStructHash(
            CHANNEL_ID, BigInteger.valueOf(100), BigInteger.valueOf(2), BigInteger.valueOf(1745500000L));
        byte[] h3 = Eip712Hashing.settleAuthStructHash(
            CHANNEL_ID, BigInteger.valueOf(100), BigInteger.valueOf(1), BigInteger.valueOf(1745500001L));
        assertThat(h1).isNotEqualTo(h2);
        assertThat(h1).isNotEqualTo(h3);
    }

    @Test
    void settle_and_close_typehashes_differ() {
        // SettleAuthorization and CloseAuthorization have identical fields but different type names —
        // typehashes MUST differ to prevent cross-type signature reuse.
        assertThat(Eip712Hashing.SETTLE_AUTH_TYPEHASH)
            .isNotEqualTo(Eip712Hashing.CLOSE_AUTH_TYPEHASH);
    }

    @Test
    void digest_combines_domain_and_struct_hash() {
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.defaults();
        byte[] dom = Eip712Hashing.domainSeparator(d);
        byte[] struct = Eip712Hashing.voucherStructHash(CHANNEL_ID, BigInteger.valueOf(100));
        byte[] digest = Eip712Hashing.digest(dom, struct);
        assertThat(digest).hasSize(32);
        // Re-derivation matches
        assertThat(Eip712Hashing.digest(dom, struct)).isEqualTo(digest);
    }
}
