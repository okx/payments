"""Pydantic models for EVM charge request, credential payload, and receipt.

charge payload structures.
"""

from __future__ import annotations

from pydantic import BaseModel, Field


class ChargeSplit(BaseModel):
    """A split payment entry for charge requests."""

    amount: str
    recipient: str
    memo: str | None = None


class ChargeMethodDetails(BaseModel):
    """EVM-specific fields in a charge challenge's methodDetails."""

    chain_id: int | None = Field(default=None, alias="chainId")
    fee_payer: bool | None = Field(default=None, alias="feePayer")
    memo: str | None = None
    splits: list[ChargeSplit] | None = None

    model_config = {"populate_by_name": True}

    @property
    def is_fee_payer(self) -> bool:
        return self.fee_payer is True


class ChargeRequest(BaseModel):
    """Decoded charge request from challenge.request (base64url JSON)."""

    amount: str
    currency: str
    recipient: str
    description: str | None = None
    external_id: str | None = Field(default=None, alias="externalId")
    method_details: ChargeMethodDetails | None = Field(default=None, alias="methodDetails")

    model_config = {"populate_by_name": True}


class Eip3009Authorization(BaseModel):
    """Signed EIP-3009 transferWithAuthorization for charge settle."""

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


class ChargeTransactionPayload(BaseModel):
    """Credential payload for charge transaction mode (server-pays, EIP-3009)."""

    type: str = "transaction"
    authorization: Eip3009Authorization


class ChargeHashPayload(BaseModel):
    """Credential payload for charge hash mode (client-pays)."""

    type: str = "hash"
    hash: str


class ChargeReceipt(BaseModel):
    """Receipt returned by SA API after successful charge settlement."""

    method: str = ""
    reference: str = ""
    status: str = ""
    timestamp: str = ""
    chain_id: int = Field(default=0, alias="chainId")
    challenge_id: str = Field(default="", alias="challengeId")
    external_id: str | None = Field(default=None, alias="externalId")

    model_config = {"populate_by_name": True}
