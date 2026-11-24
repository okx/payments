package com.okx.payments.mpp.voucher;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class EvmPaymentChannelDomainTest {

    @Test
    void defaults_match_sa_repo_constants() {
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.defaults();
        assertThat(d.name()).isEqualTo("EVM Payment Channel");
        assertThat(d.version()).isEqualTo("1");
        assertThat(d.chainId()).isEqualTo(196L);
        assertThat(d.escrowAddress()).isEqualToIgnoringCase("0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b");
    }

    @Test
    void env_var_overrides_each_field() {
        Map<String, String> env = Map.of(
            "MPP_DOMAIN_NAME", "Custom Domain",
            "MPP_DOMAIN_VERSION", "2",
            "MPP_CHAIN_ID", "1",
            "MPP_ESCROW_ADDRESS", "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1");
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.fromEnv(env::get);
        assertThat(d.name()).isEqualTo("Custom Domain");
        assertThat(d.version()).isEqualTo("2");
        assertThat(d.chainId()).isEqualTo(1L);
        assertThat(d.escrowAddress()).isEqualToIgnoringCase("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1");
    }

    @Test
    void env_partial_override_keeps_remaining_defaults() {
        Map<String, String> env = Map.of("MPP_CHAIN_ID", "10");
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.fromEnv(env::get);
        assertThat(d.chainId()).isEqualTo(10L);
        assertThat(d.name()).isEqualTo("EVM Payment Channel");
        assertThat(d.version()).isEqualTo("1");
    }

    @Test
    void empty_env_falls_back_to_defaults() {
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.fromEnv(k -> null);
        assertThat(d).isEqualTo(EvmPaymentChannelDomain.defaults());
    }

    @Test
    void blank_env_value_falls_back_to_default() {
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.fromEnv(
            k -> "MPP_DOMAIN_NAME".equals(k) ? "  " : null);
        assertThat(d.name()).isEqualTo("EVM Payment Channel");
    }

    @Test
    void builder_priority_takes_precedence() {
        // Builder is the highest tier — env-var doesn't apply when builder is used directly.
        EvmPaymentChannelDomain d = EvmPaymentChannelDomain.builder()
            .name("Override")
            .build();
        assertThat(d.name()).isEqualTo("Override");
        assertThat(d.chainId()).isEqualTo(196L);    // unchanged default
    }

    @Test
    void rejects_invalid_escrow_address() {
        assertThatThrownBy(() -> EvmPaymentChannelDomain.builder().escrowAddress("not-an-address").build())
            .isInstanceOf(IllegalArgumentException.class);
    }

    @Test
    void equals_is_address_case_insensitive() {
        EvmPaymentChannelDomain a = EvmPaymentChannelDomain.builder()
            .escrowAddress("0x5E550002E64FAF79B41D89FE8439EEB1BE66CE3B").build();
        EvmPaymentChannelDomain b = EvmPaymentChannelDomain.builder()
            .escrowAddress("0x5e550002e64faf79b41d89fe8439eeb1be66ce3b").build();
        assertThat(a).isEqualTo(b);
        assertThat(a.hashCode()).isEqualTo(b.hashCode());
    }
}
