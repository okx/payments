package com.okx.payments.mpp.protocol.encoding;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class HexUtilTest {

    @Test
    void encode_roundtrip() {
        byte[] in = {0x12, (byte) 0xab, 0x00, (byte) 0xff};
        String hex = HexUtil.encode(in);
        assertThat(hex).isEqualTo("0x12ab00ff");
        assertThat(HexUtil.decode(hex)).isEqualTo(in);
    }

    @Test
    void decode_with_uppercase_prefix() {
        assertThat(HexUtil.decode("0XAB")).containsExactly((byte) 0xab);
    }

    @Test
    void decode_without_prefix() {
        assertThat(HexUtil.decode("ab")).containsExactly((byte) 0xab);
    }

    @Test
    void decode_rejects_odd_length() {
        assertThatThrownBy(() -> HexUtil.decode("0xabc"))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void decode_rejects_invalid_hex() {
        assertThatThrownBy(() -> HexUtil.decode("0xzz"))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void address_pattern_matches_eip55_or_lowercase() {
        assertThat(HexUtil.requireAddress("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1", "from"))
            .isNotNull();
        assertThat(HexUtil.requireAddress("0x4B22FDBC399BD422B6FEFCBCE95F76642EA29DF1", "from"))
            .isNotNull();
    }

    @Test
    void address_pattern_rejects_wrong_length() {
        assertThatThrownBy(() -> HexUtil.requireAddress("0x4b22fdbc", "from"))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void bytes32_pattern() {
        assertThat(HexUtil.requireBytes32(
            "0x6d0f4fdf1f2f6a1f6c1b0fbd6a7d5c2c0a8d3d7b1f6a9c1b3e2d4a5b6c7d8e9f", "channelId"))
            .isNotNull();
        assertThatThrownBy(() -> HexUtil.requireBytes32("0xab", "channelId"))
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void packed_signature_pattern() {
        String sig = "0x" + "4a5b6c7d8e9fa0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5"
            + "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" + "1b";
        assertThat(HexUtil.requirePackedSignature(sig, "voucherSignature")).isEqualTo(sig);
    }

    @Test
    void packed_signature_or_empty_accepts_blank() {
        assertThat(HexUtil.requirePackedSignatureOrEmpty("", "voucherSignature")).isEmpty();
    }

    @Test
    void uint256_decimal_pattern() {
        assertThat(HexUtil.requireUint256Decimal("0", "value")).isEqualTo("0");
        assertThat(HexUtil.requireUint256Decimal(
            "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            "value")).isNotNull(); // 2^256 - 1 (78 digits)
    }

    @Test
    void uint256_decimal_rejects_negative_or_hex() {
        assertThatThrownBy(() -> HexUtil.requireUint256Decimal("-1", "value"))
            .isInstanceOf(IllegalArgumentException.class);
        assertThatThrownBy(() -> HexUtil.requireUint256Decimal("0xff", "value"))
            .isInstanceOf(IllegalArgumentException.class);
    }
}
