"""Tests for SA API error code mapping and error subclasses."""

from __future__ import annotations

import pytest

from mpp_evm.errors import (
    SA_CODE_AMOUNT_EXCEEDS_DEPOSIT,
    SA_CODE_CHALLENGE_INVALID,
    SA_CODE_CHANNEL_CLOSED,
    SA_CODE_CHANNEL_CLOSING,
    SA_CODE_CHANNEL_NOT_FOUND,
    SA_CODE_GRACE_PERIOD_TOO_SHORT,
    SA_CODE_INTERNAL_ERROR,
    SA_CODE_INVALID_CREDENTIAL,
    SA_CODE_INVALID_PARAMS,
    SA_CODE_INVALID_SIGNATURE,
    SA_CODE_PAYER_BLOCKED,
    SA_CODE_SPLIT_COUNT_EXCEEDED,
    SA_CODE_SPLIT_SUM_EXCEEDS_TOTAL,
    SA_CODE_TX_NOT_CONFIRMED,
    SA_CODE_UNSUPPORTED_CHAIN,
    SA_CODE_VOUCHER_DELTA_TOO_SMALL,
    AmountExceedsDepositError,
    BadRequestError,
    ChannelClosedError,
    ChannelNotFoundError,
    DeltaTooSmallError,
    EvmPaymentError,
    InsufficientBalanceError,
    InternalSAError,
    InvalidChallengeError,
    InvalidSignatureError,
    InvalidSplitError,
    MalformedCredentialError,
    SignerMismatchError,
    map_sa_error,
)


class TestMapSAError:
    """Tests for map_sa_error() function."""

    @pytest.mark.parametrize(
        "code,expected_cls",
        [
            (SA_CODE_INVALID_PARAMS, BadRequestError),
            (SA_CODE_UNSUPPORTED_CHAIN, InternalSAError),
            (SA_CODE_PAYER_BLOCKED, MalformedCredentialError),
            (SA_CODE_INVALID_CREDENTIAL, MalformedCredentialError),
            (SA_CODE_INVALID_SIGNATURE, InvalidSignatureError),
            (SA_CODE_SPLIT_SUM_EXCEEDS_TOTAL, InvalidSplitError),
            (SA_CODE_SPLIT_COUNT_EXCEEDED, InvalidSplitError),
            (SA_CODE_TX_NOT_CONFIRMED, InternalSAError),
            (SA_CODE_CHANNEL_CLOSED, ChannelClosedError),
            (SA_CODE_CHALLENGE_INVALID, InvalidChallengeError),
            (SA_CODE_CHANNEL_NOT_FOUND, ChannelNotFoundError),
            (SA_CODE_GRACE_PERIOD_TOO_SHORT, InternalSAError),
            (SA_CODE_AMOUNT_EXCEEDS_DEPOSIT, AmountExceedsDepositError),
            (SA_CODE_VOUCHER_DELTA_TOO_SMALL, DeltaTooSmallError),
            (SA_CODE_CHANNEL_CLOSING, ChannelClosedError),
            (SA_CODE_INTERNAL_ERROR, InternalSAError),
        ],
    )
    def test_maps_sa_code_to_correct_error(self, code: int, expected_cls: type) -> None:
        err = map_sa_error(code, "test message")
        assert isinstance(err, expected_cls)
        assert err.detail == "test message"
        assert err.reason == f"SA error code {code}"

    def test_unknown_code_maps_to_internal(self) -> None:
        err = map_sa_error(99999, "unknown")
        assert isinstance(err, InternalSAError)

    def test_error_is_exception(self) -> None:
        err = map_sa_error(SA_CODE_CHANNEL_NOT_FOUND, "not found")
        assert isinstance(err, Exception)
        assert isinstance(err, EvmPaymentError)


class TestErrorSubclasses:
    """Tests for error class attributes and RFC 9457 problem details."""

    def test_session_error_status_codes(self) -> None:
        assert InsufficientBalanceError.status_code == 402
        assert InvalidSignatureError.status_code == 402
        assert SignerMismatchError.status_code == 402
        assert AmountExceedsDepositError.status_code == 402
        assert DeltaTooSmallError.status_code == 402
        assert ChannelNotFoundError.status_code == 410
        assert ChannelClosedError.status_code == 410
        assert InternalSAError.status_code == 500

    def test_problem_details_format(self) -> None:
        err = ChannelNotFoundError(detail="Channel 0xabc not found")
        details = err.to_problem_details(challenge_id="ch-123")
        assert details["type"] == "https://paymentauth.org/problems/session/channel-not-found"
        assert details["title"] == "ChannelNotFoundError"
        assert details["status"] == 410
        assert details["detail"] == "Channel 0xabc not found"
        assert details["challengeId"] == "ch-123"

    def test_problem_details_no_challenge_id(self) -> None:
        err = InsufficientBalanceError(detail="low balance")
        details = err.to_problem_details()
        assert "challengeId" not in details
        assert details["type"] == "https://paymentauth.org/problems/session/insufficient-balance"

    def test_core_error_type_uri(self) -> None:
        err = MalformedCredentialError(detail="bad format")
        details = err.to_problem_details()
        assert details["type"] == "https://paymentauth.org/problems/malformed-credential"
        assert details["status"] == 402

    def test_internal_error_type_uri(self) -> None:
        err = InternalSAError(detail="server error")
        details = err.to_problem_details()
        assert details["type"] == "https://paymentauth.org/problems/internal-error"
        assert details["status"] == 500

    def test_error_str_representation(self) -> None:
        err = DeltaTooSmallError(detail="delta 5 < min 30")
        assert str(err) == "delta 5 < min 30"

    def test_error_without_detail(self) -> None:
        err = ChannelClosedError()
        assert str(err) == "ChannelClosedError"
        details = err.to_problem_details()
        assert details["detail"] == "ChannelClosedError"


class TestErrorConstants:
    """Verify SA error code constant values match the spec."""

    def test_sa_code_values(self) -> None:
        assert SA_CODE_INVALID_PARAMS == 70000
        assert SA_CODE_UNSUPPORTED_CHAIN == 70001
        assert SA_CODE_PAYER_BLOCKED == 70002
        assert SA_CODE_INVALID_CREDENTIAL == 70003
        assert SA_CODE_INVALID_SIGNATURE == 70004
        assert SA_CODE_SPLIT_SUM_EXCEEDS_TOTAL == 70005
        assert SA_CODE_SPLIT_COUNT_EXCEEDED == 70006
        assert SA_CODE_TX_NOT_CONFIRMED == 70007
        assert SA_CODE_CHANNEL_CLOSED == 70008
        assert SA_CODE_CHALLENGE_INVALID == 70009
        assert SA_CODE_CHANNEL_NOT_FOUND == 70010
        assert SA_CODE_GRACE_PERIOD_TOO_SHORT == 70011
        assert SA_CODE_AMOUNT_EXCEEDS_DEPOSIT == 70012
        assert SA_CODE_VOUCHER_DELTA_TOO_SMALL == 70013
        assert SA_CODE_CHANNEL_CLOSING == 70014
        assert SA_CODE_INTERNAL_ERROR == 8000
