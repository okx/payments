"""EVM charge intent — server-side payment verification.

Implements the pympp Intent protocol for EVM charge payments.
Dispatches credentials to the SA API for settlement (transaction mode)
or hash verification (hash mode).

"""

from __future__ import annotations

from typing import Any

from mpp import Credential, Receipt
from mpp.errors import VerificationError

from mpp_evm._defaults import METHOD_NAME_EVM, X_LAYER_CHAIN_ID
from mpp_evm.errors import InvalidPayloadError
from mpp_evm.saclient.client import SAClient
from mpp_evm.saclient.types import (
    ChallengeEcho,
    ChargeHashPayload as SAChargeHashPayload,
    ChargeSettleRequest,
    ChargeTransactionPayload as SAChargeTransactionPayload,
    ChargeVerifyHashRequest,
    Eip3009Authorization as SAEip3009Authorization,
)


class ChargeIntent:
    """EVM charge intent for one-time payments via SA API.

    Verifies payment credentials by forwarding to the OKX Settlement Agent:
    - transaction mode: EIP-3009 authorization → SA /charge/settle
    - hash mode: pre-broadcast tx hash → SA /charge/verifyHash

    Implements pympp Intent protocol (name + async verify).
    """

    name: str = "charge"

    def __init__(
        self,
        *,
        sa_client: SAClient,
        chain_id: int = X_LAYER_CHAIN_ID,
        recipient: str = "",
        fee_payer: bool = False,
    ) -> None:
        self._sa_client = sa_client
        self._chain_id = chain_id
        self._recipient = recipient
        self._fee_payer = fee_payer

    def challenge_method_details(self) -> dict[str, Any]:
        """EVM-specific fields for the challenge request."""
        return {"chainId": self._chain_id, "feePayer": self._fee_payer}

    async def verify(self, credential: Credential, request: dict[str, Any]) -> Receipt:
        """Verify an EVM charge credential.

        Dispatches based on credential.payload["type"]:
          - "transaction" → SA API /charge/settle (server-pays, EIP-3009)
          - "hash"        → SA API /charge/verifyHash (client-pays)

        Args:
            credential: Payment credential from the client Authorization header.
            request: Original charge request parameters (decoded from challenge).

        Returns:
            Receipt with transaction reference on success.

        Raises:
            VerificationError: On missing/invalid credential fields.
            EvmPaymentError subclass: On SA API errors (mapped from SA codes).
        """
        payload = credential.payload
        if not isinstance(payload, dict):
            raise VerificationError("Invalid credential payload: not a dict")

        payload_type = payload.get("type")
        if payload_type == "transaction":
            return await self._handle_transaction(credential, request)
        elif payload_type == "hash":
            return await self._handle_hash(credential, request)
        else:
            raise VerificationError(f"Unsupported charge payload type: {payload_type!r}")

    async def _handle_transaction(
        self, credential: Credential, request: dict[str, Any]
    ) -> Receipt:
        """EIP-3009 server-pays flow: submit authorization to SA for on-chain execution.

        """
        payload = credential.payload
        authorization = payload.get("authorization")
        if not authorization or not isinstance(authorization, dict):
            raise VerificationError("Missing or invalid authorization in transaction payload")

        # Build SA API authorization model
        splits_raw = authorization.get("splits")
        sa_splits = None
        if splits_raw and isinstance(splits_raw, list):
            sa_splits = [
                SAEip3009Authorization(
                    type=s.get("type", "eip-3009"),
                    from_address=s.get("from", ""),
                    to=s.get("to", ""),
                    value=s.get("value", ""),
                    valid_after=s.get("validAfter", ""),
                    valid_before=s.get("validBefore", ""),
                    nonce=s.get("nonce", ""),
                    signature=s.get("signature"),
                )
                for s in splits_raw
            ]

        sa_authorization = SAEip3009Authorization(
            type=authorization.get("type", "eip-3009"),
            from_address=authorization.get("from", ""),
            to=authorization.get("to", ""),
            value=authorization.get("value", ""),
            valid_after=authorization.get("validAfter", ""),
            valid_before=authorization.get("validBefore", ""),
            nonce=authorization.get("nonce", ""),
            signature=authorization.get("signature"),
            splits=sa_splits,
        )

        # Build challenge echo for SA request
        challenge_echo = self._build_challenge_echo(credential)

        sa_req = ChargeSettleRequest(
            challenge=challenge_echo,
            payload=SAChargeTransactionPayload(
                type="transaction",
                authorization=sa_authorization,
            ),
            source=credential.source or "",
        )

        sa_receipt = await self._sa_client.settle(sa_req)

        return Receipt.success(
            reference=sa_receipt.reference,
            method=METHOD_NAME_EVM,
            external_id=sa_receipt.external_id or None,
        )

    async def _handle_hash(self, credential: Credential, request: dict[str, Any]) -> Receipt:
        """Client-pays hash flow: verify a pre-broadcast transaction hash.

        """
        payload = credential.payload
        tx_hash = payload.get("hash")
        if not tx_hash or not isinstance(tx_hash, str):
            raise VerificationError("Missing or invalid hash in hash payload")

        challenge_echo = self._build_challenge_echo(credential)

        sa_req = ChargeVerifyHashRequest(
            challenge=challenge_echo,
            payload=SAChargeHashPayload(type="hash", hash=tx_hash),
            source=credential.source or "",
        )

        sa_receipt = await self._sa_client.verify_hash(sa_req)

        return Receipt.success(
            reference=sa_receipt.reference,
            method=METHOD_NAME_EVM,
            external_id=sa_receipt.external_id or None,
        )

    def _build_challenge_echo(self, credential: Credential) -> ChallengeEcho | None:
        """Build SA API ChallengeEcho from pympp credential's challenge field."""
        echo = credential.challenge
        if echo is None:
            return None
        return ChallengeEcho(
            id=echo.id,
            realm=echo.realm,
            method=echo.method,
            intent=echo.intent,
            request=echo.request,
            expires=echo.expires,
        )
