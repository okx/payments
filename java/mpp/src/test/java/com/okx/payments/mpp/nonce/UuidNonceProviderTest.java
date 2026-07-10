package com.okx.payments.mpp.nonce;

import org.junit.jupiter.api.Test;

import java.math.BigInteger;
import java.util.HashSet;
import java.util.Set;

import static org.assertj.core.api.Assertions.assertThat;

class UuidNonceProviderTest {

    @Test
    void allocate_produces_uint256_within_range() {
        BigInteger n = new UuidNonceProvider().allocate("payee", "channel");
        assertThat(n.signum()).isPositive();
        assertThat(n.bitLength()).isLessThanOrEqualTo(256);
    }

    @Test
    void successive_allocations_are_unique() {
        UuidNonceProvider p = new UuidNonceProvider();
        Set<BigInteger> seen = new HashSet<>();
        for (int i = 0; i < 1000; i++) {
            BigInteger n = p.allocate("p", "c");
            assertThat(seen).doesNotContain(n);
            seen.add(n);
        }
    }

    @Test
    void decimal_string_form_fits_uint256_regex() {
        for (int i = 0; i < 100; i++) {
            String s = new UuidNonceProvider().allocate("p", "c").toString();
            assertThat(s).matches("\\d{1,78}");
        }
    }
}
