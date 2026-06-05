"""EVM charge method — implements pympp Method protocol for charge payments.

Provides the server-side Method that registers the ChargeIntent and
can generate challenge parameters for EVM charge flows.


combined with pympp's tempo/client.py TempoMethod pattern.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any

from mpp import Challenge, Credential

from mpp_evm._defaults import METHOD_NAME_EVM, X_LAYER_CHAIN_ID
from mpp_evm.charge.intent import ChargeIntent
from mpp_evm.saclient.client import SAClient

if TYPE_CHECKING:
    from mpp.server.intent import Intent


@dataclass
class EvmChargeMethod:
    """EVM charge payment method.

    Implements the pympp Method protocol for EVM charge payments.
    Registers a ChargeIntent for server-side verification.

    Example:
        from mpp_evm.charge import EvmChargeMethod, ChargeIntent
        from mpp_evm.saclient import OKXSAClient

        sa_client = OKXSAClient(base_url=..., api_key=..., secret_key=..., passphrase=...)
        method = EvmChargeMethod(
            sa_client=sa_client,
            chain_id=196,
            recipient="0x742d...",
            fee_payer=True,
        )
    """

    name: str = METHOD_NAME_EVM

    sa_client: SAClient | None = None
    chain_id: int = X_LAYER_CHAIN_ID
    recipient: str = ""
    fee_payer: bool = False

    _intents: dict[str, Any] = field(default_factory=dict, init=False, repr=False)
    _charge_intent: ChargeIntent | None = field(default=None, init=False, repr=False)

    def __post_init__(self) -> None:
        if self.sa_client is not None:
            self._charge_intent = ChargeIntent(
                sa_client=self.sa_client,
                chain_id=self.chain_id,
                recipient=self.recipient,
                fee_payer=self.fee_payer,
            )
            self._intents = {"charge": self._charge_intent}

    @property
    def intents(self) -> dict[str, Any]:
        """Available intents (charge only for this method)."""
        return self._intents

    @property
    def charge_intent(self) -> ChargeIntent | None:
        """Direct access to the charge intent."""
        return self._charge_intent

    def challenge_method_details(self) -> dict[str, Any]:
        """Generate methodDetails for inclusion in a charge challenge.

        Returns the EVM-specific fields that inform the client how to
        construct a valid credential.
        """
        return {"chainId": self.chain_id, "feePayer": self.fee_payer}

    async def create_credential(self, challenge: Challenge) -> Credential:
        """Client-side credential creation (not implemented for server-only use).

        The EVM charge method on the server side does not create credentials.
        Client implementations would sign EIP-3009 authorizations here.

        Raises:
            NotImplementedError: Always, as this is a server-side method.
        """
        raise NotImplementedError(
            "EvmChargeMethod is server-side only; "
            "client credential creation requires an EVM wallet/signer"
        )
