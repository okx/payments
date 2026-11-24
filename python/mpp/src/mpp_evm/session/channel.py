"""Channel state management.

Uses pympp Store protocol for persistence (any backend: memory, redis, sqlite).
"""

from __future__ import annotations

import base64
import re
from dataclasses import dataclass, field, asdict
from typing import Any, Protocol, runtime_checkable


def _hex_to_base64(hex_str: str) -> str:
    """Convert 0x-prefixed hex to standard base64 (standard base64)."""
    if not hex_str:
        return ""
    raw = bytes.fromhex(hex_str.removeprefix("0x"))
    return base64.standard_b64encode(raw).decode()


def _base64_to_hex(b64_str: str) -> str:
    """Convert base64 to 0x-prefixed hex."""
    if not b64_str:
        return ""
    raw = base64.standard_b64decode(b64_str)
    return "0x" + raw.hex()


def _is_hex(s: str) -> bool:
    """Check if string looks like 0x-prefixed hex."""
    return bool(s) and s.startswith("0x") and all(c in "0123456789abcdefABCDEF" for c in s[2:])


@runtime_checkable
class ChannelStore(Protocol):
    """Async key-value store protocol for channel state."""

    async def get(self, key: str) -> Any | None: ...
    async def put(self, key: str, value: Any) -> None: ...
    async def delete(self, key: str) -> None: ...


# Snake-to-camel mapping matching JSON tags.
_CAMEL_MAP: dict[str, str] = {
    "channel_id": "channelId",
    "chain_id": "chainId",
    "escrow_contract": "escrowContract",
    "authorized_signer": "authorizedSigner",
    "highest_voucher_amount": "highestVoucherAmount",
    "highest_voucher_signature": "highestVoucherSignature",
    "min_voucher_delta": "minVoucherDelta",
    "close_requested_at": "closeRequestedAt",
    "created_at": "createdAt",
}
_SNAKE_MAP: dict[str, str] = {v: k for k, v in _CAMEL_MAP.items()}


@dataclass
class ChannelState:
    """Full state of a payment channel.

    All amounts stored as decimal integer strings for JSON safety.
    """

    channel_id: str
    chain_id: int
    escrow_contract: str
    payer: str
    payee: str
    token: str = ""
    authorized_signer: str = ""
    deposit: str = "0"
    highest_voucher_amount: str = "0"
    highest_voucher_signature: str = ""
    min_voucher_delta: str = "0"
    spent: str = "0"
    units: int = 0
    finalized: bool = False
    close_requested_at: int = 0
    created_at: str = ""

    # Fields that serialized as JSON numbers.
    _INT_FIELDS = {"deposit", "highest_voucher_amount", "min_voucher_delta", "spent"}
    # Fields that serialized as base64.
    _SIG_FIELDS = {"highest_voucher_signature"}

    def to_dict(self) -> dict[str, Any]:
        """Serialize with camelCase keys for cross-language compatibility.

        Amount fields are written as integers (not strings) to match big integer.
        Signature fields are written as base64 to match standard base64 encoding.
        """
        raw = asdict(self)
        out: dict[str, Any] = {}
        for k, v in raw.items():
            camel = _CAMEL_MAP.get(k, k)
            if k in self._INT_FIELDS:
                out[camel] = int(v)
            elif k in self._SIG_FIELDS and _is_hex(v):
                out[camel] = _hex_to_base64(v)
            else:
                out[camel] = v
        return out

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "ChannelState":
        known_fields = {f.name for f in cls.__dataclass_fields__.values()}
        # Accept both camelCase (from Go) and snake_case keys.
        normalized = {_SNAKE_MAP.get(k, k): v for k, v in data.items()}
        # Amount fields may be numbers; convert to str.
        for f in cls._INT_FIELDS:
            if f in normalized and not isinstance(normalized[f], str):
                normalized[f] = str(normalized[f])
        # Signature fields may be base64; convert to 0x-hex.
        for f in cls._SIG_FIELDS:
            val = normalized.get(f, "")
            if val and not _is_hex(val):
                normalized[f] = _base64_to_hex(val)
        filtered = {k: v for k, v in normalized.items() if k in known_fields}
        return cls(**filtered)

    @property
    def available_balance(self) -> int:
        """Available = highestVoucherAmount - spent."""
        return int(self.highest_voucher_amount) - int(self.spent)


async def deduct_from_channel(
    store: ChannelStore,
    channel_id: str,
    amount: int,
) -> ChannelState:
    """Atomically deduct amount from channel's available balance.

    Caller must hold per-channel lock for atomicity.

    Raises:
        ChannelNotFoundError: channel not in store
        ChannelClosedError: channel finalized or close requested
        InsufficientBalanceError: available < amount
    """
    from mpp_evm.errors import (
        ChannelClosedError,
        ChannelNotFoundError,
        InsufficientBalanceError,
    )

    data = await store.get(channel_id)
    if data is None:
        raise ChannelNotFoundError(detail=f"channel {channel_id!r} not found")

    cs = ChannelState.from_dict(data) if isinstance(data, dict) else data
    if cs.finalized:
        raise ChannelClosedError(detail=f"channel {channel_id!r} is finalized")
    if cs.close_requested_at != 0:
        raise ChannelClosedError(detail=f"channel {channel_id!r} has a pending close request")

    available = cs.available_balance
    if available < amount:
        raise InsufficientBalanceError(
            detail=f"insufficient balance in channel {channel_id!r}: available {available}, requested {amount}"
        )

    cs.spent = str(int(cs.spent) + amount)
    cs.units += 1
    await store.put(channel_id, cs.to_dict())
    return cs
