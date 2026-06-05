"""Signer protocol and PrivateKeySigner implementation.

Uses eth_account for key management and signing.
"""

from __future__ import annotations

from typing import Protocol, runtime_checkable

from eth_account import Account
from eth_account.signers.local import LocalAccount


@runtime_checkable
class Signer(Protocol):
    """Protocol for EIP-712 message signing.

    """

    def sign(self, msg_hash: bytes) -> bytes:
        """Sign a 32-byte hash. Returns 65-byte signature (r32 || s32 || v1), v in {27,28}."""
        ...

    @property
    def address(self) -> str:
        """Return the signer's Ethereum address (0x-prefixed, checksummed)."""
        ...


class PrivateKeySigner:
    """Signer backed by an ECDSA private key via eth-account.

    """

    def __init__(self, account: LocalAccount) -> None:
        self._account = account

    @classmethod
    def from_hex(cls, hex_key: str) -> "PrivateKeySigner":
        """Create from hex-encoded private key (with or without 0x prefix)."""
        account = Account.from_key(hex_key)
        return cls(account)

    def sign(self, msg_hash: bytes) -> bytes:
        """Sign raw 32-byte hash. Returns 65 bytes (r32 || s32 || v1), v in {27,28}."""
        sig_obj = self._account.unsafe_sign_hash(msg_hash)
        r = sig_obj.r.to_bytes(32, "big")
        s = sig_obj.s.to_bytes(32, "big")
        v = bytes([sig_obj.v])
        return r + s + v

    @property
    def address(self) -> str:
        return self._account.address
