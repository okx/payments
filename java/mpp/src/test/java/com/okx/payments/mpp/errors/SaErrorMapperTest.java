package com.okx.payments.mpp.errors;

import org.junit.jupiter.api.Test;

import static org.assertj.core.api.Assertions.assertThat;

class SaErrorMapperTest {

    @Test
    void all_known_codes_map() {
        assertThat(SaErrorMapper.map(30001, "x")).isInstanceOf(BadRequestError.class);
        assertThat(SaErrorMapper.map(70000, "x")).isInstanceOf(BadRequestError.class);
        assertThat(SaErrorMapper.map(70001, "x")).isInstanceOf(UnsupportedChainError.class);
        assertThat(SaErrorMapper.map(70002, "x")).isInstanceOf(PayerBlockedError.class);
        assertThat(SaErrorMapper.map(70003, "x")).isInstanceOf(InvalidPayloadError.class);
        assertThat(SaErrorMapper.map(70004, "x")).isInstanceOf(InvalidSignatureError.class);
        assertThat(SaErrorMapper.map(70005, "x")).isInstanceOf(SplitSumExceedsTotalError.class);
        assertThat(SaErrorMapper.map(70006, "x")).isInstanceOf(SplitCountExceededError.class);
        assertThat(SaErrorMapper.map(70007, "x")).isInstanceOf(TxNotConfirmedError.class);
        assertThat(SaErrorMapper.map(70008, "x")).isInstanceOf(ChannelClosedError.class);
        assertThat(SaErrorMapper.map(70009, "x")).isInstanceOf(ChallengeInvalidError.class);
        assertThat(SaErrorMapper.map(70010, "x")).isInstanceOf(ChannelNotFoundError.class);
        assertThat(SaErrorMapper.map(70011, "x")).isInstanceOf(GracePeriodTooShortError.class);
        assertThat(SaErrorMapper.map(70012, "x")).isInstanceOf(AmountExceedsDepositError.class);
        assertThat(SaErrorMapper.map(70013, "x")).isInstanceOf(VoucherDeltaTooSmallError.class);
        assertThat(SaErrorMapper.map(70014, "x")).isInstanceOf(ChannelClosingError.class);
        assertThat(SaErrorMapper.map(70015, "x")).isInstanceOf(InsufficientBalanceError.class);
        assertThat(SaErrorMapper.map(8000, "x")).isInstanceOf(ServiceError.class);
    }

    @Test
    void unknown_code_falls_back_to_service_error_with_context_message() {
        MppError e = SaErrorMapper.map(99999, "unexpected");
        assertThat(e).isInstanceOf(ServiceError.class);
        assertThat(e.getMessage()).contains("99999").contains("unexpected");
    }

    @Test
    void unmapped_70xxx_code_falls_back_to_service_error() {
        // Code in MPP error range but not yet mapped — should fall back
        // to ServiceError; SaErrorMapper internally warns (not asserted here
        // to keep the test logger-agnostic).
        MppError e = SaErrorMapper.map(70999, "future code");
        assertThat(e).isInstanceOf(ServiceError.class);
        assertThat(e.getMessage()).contains("70999").contains("future code");
    }

    @Test
    void preserves_detail_message() {
        MppError e = SaErrorMapper.map(70004, "voucher signature mismatch");
        assertThat(e.getMessage()).isEqualTo("voucher signature mismatch");
    }
}
