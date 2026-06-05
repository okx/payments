"""EVM-specific Pydantic models for charge and session payloads.

"""

from __future__ import annotations

from pydantic import BaseModel, Field


# ---------------------------------------------------------------------------
# Charge types
# ---------------------------------------------------------------------------


class ChargeSplit(BaseModel):
    """A split payment entry for charge requests."""

    amount: str
    recipient: str
    memo: str | None = None


class EVMMethodDetails(BaseModel):
    """EVM-specific fields in a charge request's methodDetails."""

    chain_id: int | None = Field(default=None, alias="chainId")
    fee_payer: bool | None = Field(default=None, alias="feePayer")
    memo: str | None = None
    splits: list[ChargeSplit] | None = None

    model_config = {"populate_by_name": True}

    @property
    def is_fee_payer(self) -> bool:
        return self.fee_payer is True


# ---------------------------------------------------------------------------
# Session types
# ---------------------------------------------------------------------------


class SessionSplit(BaseModel):
    """A bps-based split payment entry for session requests."""

    recipient: str
    bps: int = Field(ge=1, le=9999)
    memo: str | None = None


class EVMSessionMethodDetails(BaseModel):
    """EVM-specific fields in a session request's methodDetails."""

    escrow_contract: str = Field(alias="escrowContract")
    channel_id: str | None = Field(default=None, alias="channelId")
    min_voucher_delta: str | None = Field(default=None, alias="minVoucherDelta")
    chain_id: int | None = Field(default=None, alias="chainId")
    fee_payer: bool | None = Field(default=None, alias="feePayer")
    splits: list[SessionSplit] | None = None

    model_config = {"populate_by_name": True}

    @property
    def is_fee_payer(self) -> bool:
        return self.fee_payer is True


# ---------------------------------------------------------------------------
# Credential payload types (client → server)
# ---------------------------------------------------------------------------


class OpenAuthorization(BaseModel):
    """EIP-3009 authorization fields in an open payload."""

    from_address: str = Field(alias="from")
    value: str

    model_config = {"populate_by_name": True}


class OpenPayload(BaseModel):
    """Credential payload for opening a payment channel."""

    action: str = "open"
    type: str
    channel_id: str = Field(alias="channelId")
    salt: str
    cumulative_amount: str = Field(alias="cumulativeAmount")
    signature: str
    authorization: OpenAuthorization | None = None
    hash: str | None = None
    voucher_signature: str | None = Field(default=None, alias="voucherSignature")
    authorized_signer: str | None = Field(default=None, alias="authorizedSigner")
    deposit: str | None = None

    model_config = {"populate_by_name": True}

    @property
    def effective_deposit(self) -> str:
        if self.deposit:
            return self.deposit
        if self.authorization:
            return self.authorization.value
        return ""

    def validate_fields(self) -> str | None:
        if self.type not in ("transaction", "hash"):
            return f'open: invalid type "{self.type}" (want "transaction" or "hash")'
        if not self.channel_id:
            return 'open: missing required field "channelId"'
        if not self.salt:
            return 'open: missing required field "salt"'
        if not self.cumulative_amount:
            return 'open: missing required field "cumulativeAmount"'
        if not self.signature:
            return 'open: missing required field "signature"'
        if self.type == "hash" and not self.hash:
            return 'open: hash mode requires "hash" field'
        if self.type == "transaction":
            if not self.authorization:
                return 'open: transaction mode requires "authorization"'
            if not self.authorization.from_address:
                return 'open: transaction mode requires "authorization.from"'
            if not self.voucher_signature:
                return 'open: transaction mode requires "voucherSignature"'
        return None


class TopUpPayload(BaseModel):
    """Credential payload for topping up an existing channel."""

    action: str = "topUp"
    type: str
    channel_id: str = Field(alias="channelId")
    additional_deposit: str = Field(alias="additionalDeposit")
    authorization: OpenAuthorization | None = None
    hash: str | None = None
    signature: str | None = None
    top_up_salt: str | None = Field(default=None, alias="topUpSalt")

    model_config = {"populate_by_name": True}

    def validate_fields(self) -> str | None:
        if self.type not in ("transaction", "hash"):
            return f'topUp: invalid type "{self.type}" (want "transaction" or "hash")'
        if not self.channel_id:
            return 'topUp: missing required field "channelId"'
        if not self.additional_deposit:
            return 'topUp: missing required field "additionalDeposit"'
        if self.type == "hash" and not self.hash:
            return 'topUp: hash mode requires "hash" field'
        if self.type == "transaction":
            if not self.authorization:
                return 'topUp: transaction mode requires "authorization"'
            if not self.signature:
                return 'topUp: transaction mode requires "signature"'
            if not self.top_up_salt:
                return 'topUp: transaction mode requires "topUpSalt"'
        return None


class VoucherPayload(BaseModel):
    """Credential payload for an incremental payment voucher."""

    action: str = "voucher"
    channel_id: str = Field(alias="channelId")
    cumulative_amount: str = Field(alias="cumulativeAmount")
    signature: str

    model_config = {"populate_by_name": True}

    def validate_fields(self) -> str | None:
        if not self.channel_id:
            return 'voucher: missing required field "channelId"'
        if not self.cumulative_amount:
            return 'voucher: missing required field "cumulativeAmount"'
        if not self.signature:
            return 'voucher: missing required field "signature"'
        return None


class ClosePayload(BaseModel):
    """Credential payload for closing a payment channel."""

    action: str = "close"
    channel_id: str = Field(alias="channelId")
    cumulative_amount: str = Field(alias="cumulativeAmount")
    signature: str

    model_config = {"populate_by_name": True}

    def validate_fields(self) -> str | None:
        if not self.channel_id:
            return 'close: missing required field "channelId"'
        if not self.cumulative_amount:
            return 'close: missing required field "cumulativeAmount"'
        if not self.signature:
            return 'close: missing required field "signature"'
        return None


# ---------------------------------------------------------------------------
# Charge request (decoded from challenge.request base64url JSON)
# ---------------------------------------------------------------------------


class ChargeRequest(BaseModel):
    """Decoded charge request from challenge.request field."""

    amount: str
    currency: str
    recipient: str
    description: str | None = None
    external_id: str | None = Field(default=None, alias="externalId")
    method_details: EVMMethodDetails | None = Field(default=None, alias="methodDetails")

    model_config = {"populate_by_name": True}


# ---------------------------------------------------------------------------
# Session request (decoded from challenge.request base64url JSON)
# ---------------------------------------------------------------------------


class SessionRequest(BaseModel):
    """Decoded session request from challenge.request field."""

    amount: str
    unit_type: str | None = Field(default=None, alias="unitType")
    currency: str
    recipient: str | None = None
    suggested_deposit: str | None = Field(default=None, alias="suggestedDeposit")
    description: str | None = None
    external_id: str | None = Field(default=None, alias="externalId")
    method_details: EVMSessionMethodDetails | None = Field(default=None, alias="methodDetails")

    model_config = {"populate_by_name": True}
