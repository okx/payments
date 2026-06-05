"""EVM session-specific error subclasses and SA API error code mapping.

New error subclasses extend pympp's PaymentError base.
"""

from __future__ import annotations


# ---------------------------------------------------------------------------
# Base class (standalone, does not require pympp import at definition time)
# ---------------------------------------------------------------------------


class EvmPaymentError(Exception):
    """Base class for all EVM payment errors.

    Provides RFC 9457 Problem Details formatting. Subclasses define
    status_code and problem_type_suffix to generate the 'type' URI.
    """

    status_code: int = 402
    problem_type_suffix: str = "payment-error"

    CORE_TYPE_BASE = "https://paymentauth.org/problems/"
    SESSION_TYPE_BASE = "https://paymentauth.org/problems/session/"

    def __init__(self, detail: str = "", *, reason: str = "") -> None:
        self.detail = detail
        self.reason = reason
        super().__init__(detail or self.__class__.__name__)

    @property
    def is_session_error(self) -> bool:
        return self.problem_type_suffix.startswith("session/") or self.__class__ in _SESSION_ERRORS

    @property
    def type_uri(self) -> str:
        suffix = self.problem_type_suffix
        if suffix.startswith("session/"):
            return self.SESSION_TYPE_BASE + suffix.removeprefix("session/")
        if self.is_session_error:
            return self.SESSION_TYPE_BASE + suffix
        return self.CORE_TYPE_BASE + suffix

    @property
    def title(self) -> str:
        return self.__class__.__name__

    def to_problem_details(self, challenge_id: str = "") -> dict:
        d: dict = {
            "type": self.type_uri,
            "title": self.title,
            "status": self.status_code,
            "detail": self.detail or self.title,
        }
        if challenge_id:
            d["challengeId"] = challenge_id
        return d


# ---------------------------------------------------------------------------
# Core error subclasses (mirroring pympp's errors we don't import)
# ---------------------------------------------------------------------------


class MalformedCredentialError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "malformed-credential"


class InvalidChallengeError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "invalid-challenge"


class VerificationFailedError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "verification-failed"


class PaymentExpiredError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "payment-expired"


class InvalidPayloadError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "invalid-payload"


class BadRequestError(EvmPaymentError):
    status_code = 400
    problem_type_suffix = "bad-request"


class InvalidSplitError(EvmPaymentError):
    status_code = 400
    problem_type_suffix = "invalid-split"


# ---------------------------------------------------------------------------
# Session-specific error subclasses (EVM-only, not in pympp)
# ---------------------------------------------------------------------------


class InsufficientBalanceError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "session/insufficient-balance"


class InvalidSignatureError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "session/invalid-signature"


class SignerMismatchError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "session/signer-mismatch"


class AmountExceedsDepositError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "session/amount-exceeds-deposit"


class DeltaTooSmallError(EvmPaymentError):
    status_code = 402
    problem_type_suffix = "session/delta-too-small"


class ChannelNotFoundError(EvmPaymentError):
    status_code = 410
    problem_type_suffix = "session/channel-not-found"


class ChannelClosedError(EvmPaymentError):
    status_code = 410
    problem_type_suffix = "session/channel-finalized"


class InternalSAError(EvmPaymentError):
    status_code = 500
    problem_type_suffix = "internal-error"


_SESSION_ERRORS: set[type] = {
    InsufficientBalanceError,
    InvalidSignatureError,
    SignerMismatchError,
    AmountExceedsDepositError,
    DeltaTooSmallError,
    ChannelNotFoundError,
    ChannelClosedError,
}


# ---------------------------------------------------------------------------
# SA Error Code Constants
# ---------------------------------------------------------------------------

# Values taken verbatim from the [Pay] MPP EVM API spec error-code table
# (last revised 2026-05-06) and corroborated by the TypeScript SDK. The earlier
# Python/Go tables had 70005-70013 shifted and were missing 70000.
SA_CODE_SUCCESS: int = 0
SA_CODE_INVALID_PARAMS: int = 70000
SA_CODE_UNSUPPORTED_CHAIN: int = 70001
SA_CODE_PAYER_BLOCKED: int = 70002
SA_CODE_INVALID_CREDENTIAL: int = 70003
SA_CODE_INVALID_SIGNATURE: int = 70004
SA_CODE_SPLIT_SUM_EXCEEDS_TOTAL: int = 70005
SA_CODE_SPLIT_COUNT_EXCEEDED: int = 70006
SA_CODE_TX_NOT_CONFIRMED: int = 70007
SA_CODE_CHANNEL_CLOSED: int = 70008
SA_CODE_CHALLENGE_INVALID: int = 70009
SA_CODE_CHANNEL_NOT_FOUND: int = 70010
SA_CODE_GRACE_PERIOD_TOO_SHORT: int = 70011
SA_CODE_AMOUNT_EXCEEDS_DEPOSIT: int = 70012
SA_CODE_VOUCHER_DELTA_TOO_SMALL: int = 70013
SA_CODE_CHANNEL_CLOSING: int = 70014
SA_CODE_INTERNAL_ERROR: int = 8000


# ---------------------------------------------------------------------------
# SA code → error class mapping
# ---------------------------------------------------------------------------

_SA_ERROR_MAP: dict[int, type[EvmPaymentError]] = {
    SA_CODE_INVALID_PARAMS: BadRequestError,
    SA_CODE_UNSUPPORTED_CHAIN: InternalSAError,
    SA_CODE_PAYER_BLOCKED: MalformedCredentialError,
    SA_CODE_INVALID_CREDENTIAL: MalformedCredentialError,
    SA_CODE_INVALID_SIGNATURE: InvalidSignatureError,
    SA_CODE_SPLIT_SUM_EXCEEDS_TOTAL: InvalidSplitError,
    SA_CODE_SPLIT_COUNT_EXCEEDED: InvalidSplitError,
    SA_CODE_TX_NOT_CONFIRMED: InternalSAError,
    SA_CODE_CHANNEL_CLOSED: ChannelClosedError,
    SA_CODE_CHALLENGE_INVALID: InvalidChallengeError,
    SA_CODE_CHANNEL_NOT_FOUND: ChannelNotFoundError,
    SA_CODE_GRACE_PERIOD_TOO_SHORT: InternalSAError,
    SA_CODE_AMOUNT_EXCEEDS_DEPOSIT: AmountExceedsDepositError,
    SA_CODE_VOUCHER_DELTA_TOO_SMALL: DeltaTooSmallError,
    SA_CODE_CHANNEL_CLOSING: ChannelClosedError,
    SA_CODE_INTERNAL_ERROR: InternalSAError,
}


def map_sa_error(code: int, msg: str = "") -> EvmPaymentError:
    """Map an SA API error code to the appropriate EvmPaymentError subclass.

    Args:
        code: The SA API numeric error code (70001-70014, 8000).
        msg: The SA API error message string.

    Returns:
        An instance of the corresponding EvmPaymentError subclass.
    """
    error_cls = _SA_ERROR_MAP.get(code, InternalSAError)
    return error_cls(detail=msg, reason=f"SA error code {code}")
