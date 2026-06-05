"""Top-level EVM method — implements pympp Method protocol.

Combines charge and session intents into a single method registration.
"""

from __future__ import annotations

from typing import Any


class EvmMethod:
    """EVM payment method combining charge and session intents.

    Implements the pympp Method protocol. Server-side only — does not
    create credentials (client needs an EVM wallet/signer for that).
    """

    name: str = "evm"

    def __init__(self, *, intents: dict[str, Any] | None = None) -> None:
        self._intents: dict[str, Any] = intents or {}

    @property
    def intents(self) -> dict[str, Any]:
        return self._intents

    async def create_credential(self, challenge: Any) -> Any:
        raise NotImplementedError(
            "EvmMethod is server-side only; "
            "client credential creation requires an EVM wallet/signer"
        )
