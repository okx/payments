package com.okx.payments.mpp.channel;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class ChannelIdCalculatorTest {

    @Test
    void deterministic_for_same_inputs() {
        String c1 = compute();
        String c2 = compute();
        assertThat(c1).isEqualTo(c2);
        assertThat(c1).hasSize(66); // 0x + 64 hex
        assertThat(c1).startsWith("0x");
    }

    @Test
    void changes_when_salt_changes() {
        String c1 = ChannelIdCalculator.compute(
            "0x3e315241c67462c0dd21e290a1fa31967a8566dc",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0xaaaa1234bbbb5678cccc9012dddd3456eeee7890ffff1234aaaa5678bbbb9012",
            ChannelIdCalculator.ZERO_ADDRESS,
            "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            196L);

        String c2 = ChannelIdCalculator.compute(
            "0x3e315241c67462c0dd21e290a1fa31967a8566dc",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0xbbbb1234bbbb5678cccc9012dddd3456eeee7890ffff1234aaaa5678bbbb9012",  // different salt
            ChannelIdCalculator.ZERO_ADDRESS,
            "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            196L);

        assertThat(c1).isNotEqualTo(c2);
    }

    @Test
    void changes_when_chain_id_changes() {
        String c1 = compute();
        String c2 = ChannelIdCalculator.compute(
            "0x3e315241c67462c0dd21e290a1fa31967a8566dc",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0xaaaa1234bbbb5678cccc9012dddd3456eeee7890ffff1234aaaa5678bbbb9012",
            ChannelIdCalculator.ZERO_ADDRESS,
            "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            1L);
        assertThat(c1).isNotEqualTo(c2);
    }

    @Test
    void changes_when_escrow_changes() {
        String c1 = compute();
        String c2 = ChannelIdCalculator.compute(
            "0x3e315241c67462c0dd21e290a1fa31967a8566dc",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0xaaaa1234bbbb5678cccc9012dddd3456eeee7890ffff1234aaaa5678bbbb9012",
            ChannelIdCalculator.ZERO_ADDRESS,
            "0xeb18025208061781a287fFc2c1F31C03A24a24c0",   // different escrow
            196L);
        assertThat(c1).isNotEqualTo(c2);
    }

    @Test
    void zero_address_sentinel_is_used_for_payer_self_signing() {
        assertThat(ChannelIdCalculator.ZERO_ADDRESS)
            .isEqualTo("0x0000000000000000000000000000000000000000");
    }

    private static String compute() {
        return ChannelIdCalculator.compute(
            "0x3e315241c67462c0dd21e290a1fa31967a8566dc",
            "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
            "0x74b7F1633b89720027F6196A17a631aC6dE26d22",
            "0xaaaa1234bbbb5678cccc9012dddd3456eeee7890ffff1234aaaa5678bbbb9012",
            ChannelIdCalculator.ZERO_ADDRESS,
            "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            196L);
    }
}
