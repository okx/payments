package com.okx.payments.mpp.errors;

import org.junit.jupiter.api.Test;

import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class MppErrorTest {

    @Test
    void to_problem_details_includes_status_type_title_and_detail() {
        MppError e = new InvalidSignatureError("voucher recovery failed");
        Map<String, Object> p = e.toProblemDetails(null);
        assertThat(p.get("status")).isEqualTo(402);
        assertThat(p.get("type")).isEqualTo("https://paymentauth.org/problems/invalid-signature");
        assertThat(p.get("title")).isEqualTo("Invalid Signature");
        assertThat(p.get("detail")).isEqualTo("voucher recovery failed");
        assertThat(p).doesNotContainKey("challengeId");
    }

    @Test
    void to_problem_details_includes_challenge_id_when_provided() {
        MppError e = new InvalidPayloadError("missing source");
        Map<String, Object> p = e.toProblemDetails("qB3wErTy");
        assertThat(p.get("challengeId")).isEqualTo("qB3wErTy");
    }

    @Test
    void context_is_chainable_and_appears_in_problem_details() {
        MppError e = new ChannelNotFoundError("no channel")
            .put("channelId", "0xdead")
            .put("chainId", 196);
        @SuppressWarnings("unchecked")
        Map<String, Object> ctx = (Map<String, Object>) e.toProblemDetails(null).get("context");
        assertThat(ctx).containsEntry("channelId", "0xdead").containsEntry("chainId", 196);
    }

    @Test
    void context_skips_null_values() {
        MppError e = new ServiceError("oops").put("foo", null);
        assertThat(e.context()).isEmpty();
    }

    @Test
    void status_code_categories() {
        assertThat(new BadRequestError("x").status()).isEqualTo(400);
        assertThat(new InvalidPayloadError("x").status()).isEqualTo(402);
        assertThat(new ChannelNotFoundError("x").status()).isEqualTo(410);
        assertThat(new ServiceError("x").status()).isEqualTo(500);
    }
}
