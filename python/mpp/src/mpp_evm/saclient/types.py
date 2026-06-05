"""SA API request/response Pydantic models.

charge/session payload types, and receipt types.
"""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, Field


# ---------------------------------------------------------------------------
# Response envelope
# ---------------------------------------------------------------------------


class SAResponse(BaseModel):
    """Standard SA API response envelope."""

    code: int = 0
    msg: str = ""
    data: Any = None


# ---------------------------------------------------------------------------
# EIP-3009 authorization (shared across charge and session)
# ---------------------------------------------------------------------------


class Eip3009Authorization(BaseModel):
    """Signed EIP-3009 transferWithAuthorization."""

    type: str = "eip-3009"
    from_address: str = Field(alias="from")
    to: str
    value: str
    valid_after: str = Field(alias="validAfter")
    valid_before: str = Field(alias="validBefore")
    nonce: str
    signature: str | None = None
    splits: list["Eip3009Authorization"] | None = None

    model_config = {"populate_by_name": True}


# ---------------------------------------------------------------------------
# Challenge echo (wire format for SA requests)
# ---------------------------------------------------------------------------


class ChallengeEcho(BaseModel):
    """Challenge echo included in SA credential requests."""

    id: str
    realm: str
    method: str
    intent: str
    request: str
    expires: str | None = None
    description: str | None = None
    digest: str | None = None
    opaque: str | None = None

    model_config = {"populate_by_name": True}


# ---------------------------------------------------------------------------
# Charge payload types
# ---------------------------------------------------------------------------


class ChargeTransactionPayload(BaseModel):
    """Payload for POST /charge/settle (transaction mode)."""

    type: str = "transaction"
    authorization: Eip3009Authorization


class ChargeHashPayload(BaseModel):
    """Payload for POST /charge/verifyHash (hash mode)."""

    type: str = "hash"
    hash: str


# ---------------------------------------------------------------------------
# Session payload types (client-facing)
# ---------------------------------------------------------------------------


class SessionOpenPayload(BaseModel):
    """Payload for POST /session/open."""

    action: str = "open"
    type: str
    channel_id: str = Field(alias="channelId")
    authorization: Eip3009Authorization | None = None
    signature: str | None = None
    hash: str | None = None
    salt: str
    authorized_signer: str | None = Field(default=None, alias="authorizedSigner")

    model_config = {"populate_by_name": True}


class SessionTopUpPayload(BaseModel):
    """Payload for POST /session/topUp."""

    action: str = "topUp"
    type: str
    channel_id: str = Field(alias="channelId")
    authorization: Eip3009Authorization | None = None
    signature: str | None = None
    hash: str | None = None
    additional_deposit: str = Field(alias="additionalDeposit")
    top_up_salt: str | None = Field(default=None, alias="topUpSalt")

    model_config = {"populate_by_name": True}


# ---------------------------------------------------------------------------
# Session payload types (merchant-facing)
# ---------------------------------------------------------------------------


class SessionSettlePayload(BaseModel):
    """Payload for POST /session/settle."""

    action: str = "settle"
    channel_id: str = Field(alias="channelId")
    cumulative_amount: str = Field(alias="cumulativeAmount")
    voucher_signature: str = Field(alias="voucherSignature")
    payee_signature: str = Field(alias="payeeSignature")
    nonce: str
    deadline: str

    model_config = {"populate_by_name": True}


class SessionClosePayload(BaseModel):
    """Payload for POST /session/close."""

    action: str = "close"
    channel_id: str = Field(alias="channelId")
    cumulative_amount: str = Field(alias="cumulativeAmount")
    voucher_signature: str = Field(alias="voucherSignature")
    payee_signature: str = Field(alias="payeeSignature")
    nonce: str
    deadline: str

    model_config = {"populate_by_name": True}


# ---------------------------------------------------------------------------
# Credential request envelope (generic wrapper)
# ---------------------------------------------------------------------------


class ChargeSettleRequest(BaseModel):
    """Request envelope for POST /charge/settle."""

    challenge: ChallengeEcho | None = None
    payload: ChargeTransactionPayload
    source: str | None = None


class ChargeVerifyHashRequest(BaseModel):
    """Request envelope for POST /charge/verifyHash."""

    challenge: ChallengeEcho | None = None
    payload: ChargeHashPayload
    source: str | None = None


class SessionOpenRequest(BaseModel):
    """Request envelope for POST /session/open."""

    challenge: ChallengeEcho | None = None
    payload: SessionOpenPayload
    source: str | None = None


class SessionTopUpRequest(BaseModel):
    """Request envelope for POST /session/topUp."""

    challenge: ChallengeEcho | None = None
    payload: SessionTopUpPayload
    source: str | None = None


class SessionSettleRequest(BaseModel):
    """Request envelope for POST /session/settle."""

    payload: SessionSettlePayload


class SessionCloseRequest(BaseModel):
    """Request envelope for POST /session/close."""

    payload: SessionClosePayload


# ---------------------------------------------------------------------------
# Response types
# ---------------------------------------------------------------------------


class ChargeReceipt(BaseModel):
    """Response data for charge operations."""

    method: str = ""
    reference: str = ""
    status: str = ""
    timestamp: str = ""
    chain_id: int = Field(default=0, alias="chainId")
    challenge_id: str = Field(default="", alias="challengeId")
    external_id: str | None = Field(default=None, alias="externalId")

    model_config = {"populate_by_name": True}


class SessionReceipt(BaseModel):
    """Response data for session mutating operations."""

    method: str = ""
    intent: str = ""
    status: str = ""
    timestamp: str = ""
    channel_id: str = Field(default="", alias="channelId")
    chain_id: int = Field(default=0, alias="chainId")
    reference: str = ""
    deposit: str = ""

    model_config = {"populate_by_name": True}


class SessionStatus(BaseModel):
    """Response data for GET /session/status."""

    channel_id: str = Field(default="", alias="channelId")
    payer: str = ""
    payee: str = ""
    token: str = ""
    deposit: str = ""
    cumulative_amount: str = Field(default="", alias="cumulativeAmount")
    settled_on_chain: str = Field(default="", alias="settledOnChain")
    remaining_balance: str = Field(default="", alias="remainingBalance")
    session_status: str = Field(default="", alias="sessionStatus")

    model_config = {"populate_by_name": True}
