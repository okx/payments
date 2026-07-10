package com.okx.payments.mpp.seller;

import org.junit.jupiter.api.Test;

import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

class ChallengeSignerTest {

    private static final byte[] KEY1 = "super-secret-key-1234567890abcdef".getBytes();
    private static final byte[] KEY2 = "rotation-key-9876543210fedcba".getBytes();

    private final ChallengeSigner signer1 = new ChallengeSigner(KEY1);

    @Test
    void sign_then_verify_roundtrip() {
        String id = signer1.sign("api.example.com", "evm", "charge",
            "eyJhbW91bnQ", "2026-04-01T12:05:00Z", null, null);
        assertThat(id).isNotEmpty();
        assertThat(signer1.verify(id, "api.example.com", "evm", "charge",
            "eyJhbW91bnQ", "2026-04-01T12:05:00Z", null, null)).isTrue();
    }

    @Test
    void verify_fails_when_any_field_changes() {
        String id = signer1.sign("api.example.com", "evm", "charge",
            "eyJhbW91bnQ", "2026-04-01T12:05:00Z", null, null);
        assertThat(signer1.verify(id, "api.OTHER.com", "evm", "charge",
            "eyJhbW91bnQ", "2026-04-01T12:05:00Z", null, null)).isFalse();
        assertThat(signer1.verify(id, "api.example.com", "evm", "session",
            "eyJhbW91bnQ", "2026-04-01T12:05:00Z", null, null)).isFalse();
        assertThat(signer1.verify(id, "api.example.com", "evm", "charge",
            "DIFFERENT", "2026-04-01T12:05:00Z", null, null)).isFalse();
    }

    @Test
    void null_optional_fields_treated_as_empty_string() {
        String idA = signer1.sign("realm", "evm", "charge", "req", "exp", null, null);
        String idB = signer1.sign("realm", "evm", "charge", "req", "exp", "", "");
        assertThat(idA).isEqualTo(idB);
    }

    @Test
    void rotation_verifies_against_either_key() {
        ChallengeSigner active = new ChallengeSigner(KEY1);
        String id = active.sign("realm", "evm", "charge", "req", "exp", null, null);
        ChallengeSigner rotator = new ChallengeSigner(List.of(KEY2, KEY1));   // active=KEY2 but tries both
        assertThat(rotator.verify(id, "realm", "evm", "charge", "req", "exp", null, null)).isTrue();
    }

    @Test
    void rejects_invalid_base64_id() {
        assertThat(signer1.verify("@@@!!!", "realm", "evm", "charge", "req", "exp", null, null))
            .isFalse();
    }

    @Test
    void rejects_null_id() {
        assertThat(signer1.verify(null, "realm", "evm", "charge", "req", "exp", null, null))
            .isFalse();
    }

    @Test
    void empty_keys_throws() {
        assertThatThrownBy(() -> new ChallengeSigner(List.of()))
            .isInstanceOf(IllegalArgumentException.class);
    }
}
