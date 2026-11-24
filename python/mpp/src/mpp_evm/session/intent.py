"""EVM session intent — implements pympp Intent protocol.

handleOpen, handleTopUp, handleVoucher, handleClose, SettleWithAuthorization,
CloseWithAuthorization, and the Respond() short-circuit pattern.
"""

from __future__ import annotations

import asyncio
from datetime import datetime, timezone
from typing import Any

from mpp_evm._defaults import (
    ACTION_CLOSE,
    ACTION_OPEN,
    ACTION_SETTLE,
    ACTION_TOP_UP,
    ACTION_VOUCHER,
    DEFAULT_DOMAIN_NAME,
    DEFAULT_DOMAIN_VERSION,
    DEFAULT_ESCROW_CONTRACT,
    STATUS_CLOSED,
    STATUS_OPEN,
    X_LAYER_CHAIN_ID,
)
from mpp_evm.errors import (
    AmountExceedsDepositError,
    ChannelClosedError,
    ChannelNotFoundError,
    DeltaTooSmallError,
    InsufficientBalanceError,
    InvalidPayloadError,
    InvalidSignatureError,
)
from mpp_evm.nonce import NonceProvider, UuidNonceProvider
from mpp_evm.saclient.client import SAClient
from mpp_evm.saclient.types import (
    SessionClosePayload,
    SessionCloseRequest,
    SessionOpenPayload,
    SessionOpenRequest,
    SessionSettlePayload,
    SessionSettleRequest,
    SessionTopUpPayload,
    SessionTopUpRequest,
    ChallengeEcho,
    Eip3009Authorization,
)
from mpp_evm.session.channel import ChannelState, ChannelStore, deduct_from_channel
from mpp_evm.store import InMemoryChannelStore
from mpp_evm.session.voucher import (
    sign_authorization,
    validate_voucher_signature,
    verify_voucher_or_raise,
)
from mpp_evm.signer import Signer


# U256 MAX (no expiry default)
_U256_MAX = (2**256) - 1


def _hex_to_bytes32(s: str) -> bytes:
    """Decode hex string (with optional 0x prefix) to bytes, left-pad to 32."""
    raw = bytes.fromhex(s.removeprefix("0x"))
    if len(raw) > 32:
        raise ValueError(f"value too long: got {len(raw)} bytes, want <=32")
    return raw.rjust(32, b"\x00")


def _parse_did_pkh(source: str) -> tuple[str, int]:
    """Parse DID PKH source: did:pkh:eip155:{chainId}:{address} -> (address, chainId)."""
    parts = source.split(":")
    if len(parts) < 5 or parts[0] != "did" or parts[1] != "pkh" or parts[2] != "eip155":
        raise InvalidPayloadError(detail=f"invalid DID PKH source: {source!r}")
    chain_id = int(parts[3])
    address = parts[4].lower()
    return address, chain_id


class SessionIntent:
    """EVM session intent — full payment channel lifecycle.

    Implements pympp Intent protocol. Dispatches by credential payload action:
      open    → SA API /session/open, store channel state
      topUp   → SA API /session/topUp, increase deposit
      voucher → EIP-712 verify, deduct per-request cost
      close   → verify final voucher, sign CloseAuthorization, SA API /session/close
      settle  → sign SettleAuthorization, SA API /session/settle
    """

    name: str = "session"

    def __init__(
        self,
        *,
        sa_client: SAClient,
        recipient: str,
        signer: Signer | None = None,
        store: ChannelStore | None = None,
        chain_id: int = X_LAYER_CHAIN_ID,
        escrow_contract: str = DEFAULT_ESCROW_CONTRACT,
        per_request_cost: int | None = None,
        min_voucher_delta: int | None = None,
        nonce_provider: NonceProvider | None = None,
        deadline: int | None = None,
        domain_name: str = DEFAULT_DOMAIN_NAME,
        domain_version: str = DEFAULT_DOMAIN_VERSION,
        fee_payer: bool = False,
    ) -> None:
        self._sa_client = sa_client
        self._recipient = recipient
        self._signer = signer
        # Default to a non-persistent in-memory store when none is provided,
        # matching the Go/Rust/TS SDKs. Without this, every session handler
        # dereferences self._store unconditionally and would raise on None.
        self._store: ChannelStore = store or InMemoryChannelStore()
        self._chain_id = chain_id
        self._escrow_contract = escrow_contract
        self._per_request_cost = per_request_cost
        self._min_voucher_delta = min_voucher_delta or 0
        self._nonce_provider = nonce_provider or UuidNonceProvider()
        self._deadline = deadline
        self._domain_name = domain_name
        self._domain_version = domain_version
        self._fee_payer = fee_payer
        self._channel_locks: dict[str, asyncio.Lock] = {}

    def _get_lock(self, channel_id: str) -> asyncio.Lock:
        if channel_id not in self._channel_locks:
            self._channel_locks[channel_id] = asyncio.Lock()
        return self._channel_locks[channel_id]

    @property
    def _default_deadline(self) -> int:
        return self._deadline if self._deadline is not None else _U256_MAX

    def challenge_method_details(self) -> dict[str, Any]:
        """EVM-specific fields for the challenge request."""
        return {
            "chainId": self._chain_id,
            "escrowContract": self._escrow_contract,
            "minVoucherDelta": str(self._min_voucher_delta),
            "feePayer": self._fee_payer,
        }

    # --- Intent protocol ---

    async def verify(self, credential: Any, request: dict) -> Any:
        """Verify a session credential — dispatch by action.

        This is the pympp Intent.verify() implementation.
        """
        payload = credential.payload if hasattr(credential, "payload") else credential
        if isinstance(payload, dict):
            action = payload.get("action", "")
        else:
            action = getattr(payload, "action", "")

        if action == ACTION_OPEN:
            return await self._handle_open(credential, request)
        elif action == ACTION_TOP_UP:
            return await self._handle_top_up(credential, request)
        elif action == ACTION_VOUCHER:
            return await self._handle_voucher(credential, request)
        elif action == ACTION_CLOSE:
            return await self._handle_close(credential, request)
        elif action == ACTION_SETTLE:
            return await self._handle_settle(credential, request)
        else:
            raise InvalidPayloadError(detail=f"unknown session action: {action!r}")

    def respond(self, credential: Any, receipt: Any) -> Any:
        """Respond pattern: determines whether to serve resource or short-circuit.

        - voucher → None (proceed to resource handler)
        - open/topUp/close/settle → management response dict (terminates request)

        For management actions, wraps the session receipt into a protocol.Receipt
        with base64url-encoded settlement field, wrapping session receipt.
        """
        payload = credential.payload if hasattr(credential, "payload") else credential
        action = payload.get("action", "") if isinstance(payload, dict) else getattr(payload, "action", "")
        if action == ACTION_VOUCHER:
            return None

        # Wrap session receipt into protocol Receipt with settlement field
        import base64
        import json as _json

        challenge = getattr(credential, "challenge", None)
        challenge_id = ""
        if challenge is not None:
            challenge_id = getattr(challenge, "id", "") or ""

        settlement_data = receipt if isinstance(receipt, dict) else {}
        settlement_json = _json.dumps(settlement_data, separators=(",", ":"))
        settlement_b64 = base64.urlsafe_b64encode(settlement_json.encode()).decode().rstrip("=")

        protocol_receipt = {
            "id": challenge_id,
            "status": "success",
            "method": "evm",
            "intent": "session",
            "settlement": settlement_b64,
        }
        return {"status": "ok", "receipt": protocol_receipt}

    # --- Action handlers ---

    async def _handle_open(self, credential: Any, request: dict) -> dict:
        """Process channel open payload.

        """
        payload = self._get_payload_dict(credential)
        channel_id = payload.get("channelId", "")
        if not channel_id:
            raise InvalidPayloadError(detail='open: missing required field "channelId"')

        # Determine payer from source or authorization
        payer = self._resolve_payer(credential, payload)

        # Resolve deposit amount
        deposit_str = payload.get("deposit", "") or ""
        if not deposit_str:
            auth = payload.get("authorization")
            if auth and isinstance(auth, dict):
                deposit_str = auth.get("value", "")

        async with self._get_lock(channel_id):
            # Pre-check: no existing channel
            existing = await self._store.get(channel_id)
            if existing is not None:
                raise InvalidPayloadError(detail=f"channel {channel_id!r} already exists")

            # Call SA API /session/open
            sa_req = self._build_open_request(credential, payload)
            sa_receipt = await self._sa_client.session_open(sa_req)

            # Resolve deposit from SA receipt if not in payload
            if not deposit_str and sa_receipt.deposit:
                deposit_str = sa_receipt.deposit
            if not deposit_str:
                deposit_str = "0"

            # Resolve authorizedSigner
            authorized_signer = payload.get("authorizedSigner", "") or ""
            if not authorized_signer or authorized_signer == "0x" + "0" * 40:
                authorized_signer = payer

            # Verify initial voucher signature if present
            cumulative_amount_str = payload.get("cumulativeAmount", "0")
            cumulative_amount = int(cumulative_amount_str) if cumulative_amount_str else 0

            voucher_sig_hex = payload.get("voucherSignature", "")
            if not voucher_sig_hex and payload.get("type") == "hash":
                voucher_sig_hex = payload.get("signature", "")

            if voucher_sig_hex:
                sig_bytes = bytes.fromhex(voucher_sig_hex.removeprefix("0x"))
                validate_voucher_signature(sig_bytes)
                channel_id_bytes = _hex_to_bytes32(channel_id)
                verify_voucher_or_raise(
                    escrow_contract=self._escrow_contract,
                    chain_id=self._chain_id,
                    channel_id=channel_id_bytes,
                    cumulative_amount=cumulative_amount,
                    signature=sig_bytes,
                    expected_signer=authorized_signer,
                    domain_name=self._domain_name,
                    domain_version=self._domain_version,
                )

            # Store channel state
            token = request.get("currency", "")
            cs = ChannelState(
                channel_id=channel_id,
                chain_id=self._chain_id,
                escrow_contract=self._escrow_contract,
                payer=payer,
                payee=self._recipient,
                token=token,
                authorized_signer=authorized_signer,
                deposit=deposit_str,
                highest_voucher_amount=str(cumulative_amount),
                highest_voucher_signature=voucher_sig_hex or "",
                min_voucher_delta=str(self._min_voucher_delta),
                spent="0",
                units=0,
                finalized=False,
                created_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            )
            await self._store.put(channel_id, cs.to_dict())

        return {
            "channelId": channel_id,
            "cumulativeAmount": "0",
            "escrowContract": self._escrow_contract,
            "status": STATUS_OPEN,
            "reference": sa_receipt.reference or "",
        }

    async def _handle_top_up(self, credential: Any, request: dict) -> dict:
        """Process channel top-up payload.

        """
        payload = self._get_payload_dict(credential)
        channel_id = payload.get("channelId", "")
        additional_deposit = payload.get("additionalDeposit", "")
        if not additional_deposit:
            raise InvalidPayloadError(detail='topUp: missing "additionalDeposit"')
        add_amount = int(additional_deposit)

        async with self._get_lock(channel_id):
            cs = await self._get_channel(channel_id)

            # Call SA API /session/topUp
            sa_req = self._build_top_up_request(credential, payload)
            sa_receipt = await self._sa_client.session_top_up(sa_req)

            # Update deposit
            cs.deposit = str(int(cs.deposit) + add_amount)
            await self._store.put(channel_id, cs.to_dict())

        return {
            "channelId": channel_id,
            "cumulativeAmount": cs.highest_voucher_amount,
            "escrowContract": cs.escrow_contract,
            "status": STATUS_OPEN,
            "reference": sa_receipt.reference or "",
        }

    async def _handle_voucher(self, credential: Any, request: dict) -> dict:
        """Process voucher payload (incremental off-chain payment).

        Two paths:
          - New voucher (amount > stored): validate sig, update store
          - Replay voucher (same amount+sig): skip verification
        Then deduct perRequestCost.
        """
        payload = self._get_payload_dict(credential)
        channel_id = payload.get("channelId", "")
        cumulative_str = payload.get("cumulativeAmount", "")
        signature_hex = payload.get("signature", "")

        if not channel_id or not cumulative_str or not signature_hex:
            raise InvalidPayloadError(detail="voucher: missing required fields")

        new_amount = int(cumulative_str)
        sig_bytes = bytes.fromhex(signature_hex.removeprefix("0x"))

        async with self._get_lock(channel_id):
            cs = await self._get_channel(channel_id)

            # Amount must not exceed deposit
            deposit = int(cs.deposit)
            if new_amount > deposit:
                raise AmountExceedsDepositError(
                    detail=f"voucher amount {new_amount} exceeds deposit {deposit}"
                )

            prev_amount = int(cs.highest_voucher_amount)

            # Idempotent replay detection
            is_replay = (
                new_amount == prev_amount
                and signature_hex == cs.highest_voucher_signature
            )

            if not is_replay and new_amount <= prev_amount:
                raise DeltaTooSmallError(
                    detail=f"voucher amount {new_amount} is not strictly greater than current {prev_amount}"
                )

            if not is_replay:
                # Min delta throttle
                min_delta = self._min_voucher_delta or int(cs.min_voucher_delta or "0")
                if min_delta > 0:
                    delta = new_amount - prev_amount
                    if delta < min_delta:
                        raise DeltaTooSmallError(
                            detail=f"delta {delta} below minimum {min_delta}"
                        )

                # Signature validation
                validate_voucher_signature(sig_bytes)

                channel_id_bytes = _hex_to_bytes32(channel_id)

                # Determine expected signer
                expected_signer = cs.authorized_signer
                if not expected_signer or expected_signer == "0x" + "0" * 40:
                    expected_signer = cs.payer

                verify_voucher_or_raise(
                    escrow_contract=cs.escrow_contract,
                    chain_id=cs.chain_id,
                    channel_id=channel_id_bytes,
                    cumulative_amount=new_amount,
                    signature=sig_bytes,
                    expected_signer=expected_signer,
                    domain_name=self._domain_name,
                    domain_version=self._domain_version,
                )

                # Update store
                cs.highest_voucher_amount = str(new_amount)
                cs.highest_voucher_signature = signature_hex
                await self._store.put(channel_id, cs.to_dict())

            # Deduct per-request cost (both new and replay)
            if self._per_request_cost and self._per_request_cost > 0:
                cs = await deduct_from_channel(self._store, channel_id, self._per_request_cost)

        return {
            "channelId": channel_id,
            "cumulativeAmount": cs.highest_voucher_amount,
            "escrowContract": cs.escrow_contract,
            "status": STATUS_OPEN,
            "reference": "",
        }

    async def _handle_close(self, credential: Any, request: dict) -> dict:
        """Process channel close payload.

        Verifies client voucher, signs CloseAuthorization, calls SA, deletes channel.
        """
        payload = self._get_payload_dict(credential)
        channel_id = payload.get("channelId", "")
        cumulative_str = payload.get("cumulativeAmount", "")
        signature_hex = payload.get("signature", "")

        if not channel_id or not cumulative_str or not signature_hex:
            raise InvalidPayloadError(detail="close: missing required fields")

        client_amount = int(cumulative_str)
        client_sig = bytes.fromhex(signature_hex.removeprefix("0x"))

        async with self._get_lock(channel_id):
            cs = await self._get_channel(channel_id)
            channel_id_bytes = _hex_to_bytes32(channel_id)

            # Must not exceed deposit
            deposit = int(cs.deposit)
            if client_amount > deposit:
                raise AmountExceedsDepositError(
                    detail=f"close amount {client_amount} exceeds deposit {deposit}"
                )

            stored_amount = int(cs.highest_voucher_amount)
            if client_amount < stored_amount:
                raise DeltaTooSmallError(
                    detail=f"close amount {client_amount} less than stored {stored_amount}"
                )

            # Min delta check if higher
            if client_amount > stored_amount:
                min_delta = self._min_voucher_delta or int(cs.min_voucher_delta or "0")
                if min_delta > 0:
                    delta = client_amount - stored_amount
                    if delta < min_delta:
                        raise DeltaTooSmallError(
                            detail=f"delta {delta} below minimum {min_delta}"
                        )

            # Always verify client voucher signature
            validate_voucher_signature(client_sig)
            expected_signer = cs.authorized_signer or cs.payer
            if expected_signer == "0x" + "0" * 40:
                expected_signer = cs.payer

            verify_voucher_or_raise(
                escrow_contract=cs.escrow_contract,
                chain_id=cs.chain_id,
                channel_id=channel_id_bytes,
                cumulative_amount=client_amount,
                signature=client_sig,
                expected_signer=expected_signer,
                domain_name=self._domain_name,
                domain_version=self._domain_version,
            )

            # Update if higher
            if client_amount > stored_amount:
                cs.highest_voucher_amount = str(client_amount)
                cs.highest_voucher_signature = signature_hex
                await self._store.put(channel_id, cs.to_dict())

            # Sign CloseAuthorization and submit
            sa_receipt = await self._sign_and_submit_close(
                channel_id, channel_id_bytes,
                int(cs.highest_voucher_amount),
                cs.highest_voucher_signature,
                cs.escrow_contract, cs.chain_id,
            )

            # Delete channel from store
            await self._store.delete(channel_id)
            self._channel_locks.pop(channel_id, None)

        return {
            "channelId": channel_id,
            "cumulativeAmount": cs.highest_voucher_amount,
            "escrowContract": cs.escrow_contract,
            "status": STATUS_CLOSED,
            "reference": sa_receipt.reference if sa_receipt else "",
        }

    async def _handle_settle(self, credential: Any, request: dict) -> dict:
        """Process server-initiated intermediate settlement.

        """
        payload = self._get_payload_dict(credential)
        channel_id = payload.get("channelId", "")
        if not channel_id:
            raise InvalidPayloadError(detail='settle: missing "channelId"')

        async with self._get_lock(channel_id):
            cs = await self._get_channel(channel_id)

            cumulative_amount = int(cs.highest_voucher_amount)
            if cumulative_amount == 0:
                raise InvalidPayloadError(detail=f"channel {channel_id!r} has no voucher to settle")

            channel_id_bytes = _hex_to_bytes32(channel_id)
            voucher_sig_hex = cs.highest_voucher_signature

            # Sign SettleAuthorization
            if self._signer is None:
                raise InvalidPayloadError(detail="payee signer not configured")

            nonce = await self._nonce_provider.allocate(self._recipient, channel_id)
            deadline = self._default_deadline

            payee_sig = sign_authorization(
                signer=self._signer,
                primary_type="SettleAuthorization",
                channel_id=channel_id_bytes,
                cumulative_amount=cumulative_amount,
                nonce=nonce,
                deadline=deadline,
                escrow_contract=cs.escrow_contract,
                chain_id=cs.chain_id,
                domain_name=self._domain_name,
                domain_version=self._domain_version,
            )

            sa_req = SessionSettleRequest(
                payload=SessionSettlePayload(
                    action=ACTION_SETTLE,
                    channel_id=channel_id,
                    cumulative_amount=str(cumulative_amount),
                    voucher_signature=voucher_sig_hex,
                    payee_signature="0x" + payee_sig.hex(),
                    nonce=str(nonce),
                    deadline=str(deadline),
                )
            )
            sa_receipt = await self._sa_client.session_settle(sa_req)

        return {
            "channelId": channel_id,
            "cumulativeAmount": str(cumulative_amount),
            "escrowContract": cs.escrow_contract,
            "status": STATUS_OPEN,
            "reference": sa_receipt.reference or "",
        }

    # --- Public methods for merchant-initiated actions ---

    async def settle_channel(self, channel_id: str) -> dict:
        """Merchant-initiated intermediate settlement (no credential needed)."""
        class _FakeCredential:
            payload = {"action": ACTION_SETTLE, "channelId": channel_id}
        return await self._handle_settle(_FakeCredential(), {})

    async def close_channel(self, channel_id: str) -> dict:
        """Merchant-initiated channel close (CloseWithAuthorization pattern)."""
        async with self._get_lock(channel_id):
            cs = await self._get_channel(channel_id)
            channel_id_bytes = _hex_to_bytes32(channel_id)

            sa_receipt = await self._sign_and_submit_close(
                channel_id, channel_id_bytes,
                int(cs.highest_voucher_amount),
                cs.highest_voucher_signature,
                cs.escrow_contract, cs.chain_id,
            )

            await self._store.delete(channel_id)
            self._channel_locks.pop(channel_id, None)

        return {
            "channelId": channel_id,
            "cumulativeAmount": cs.highest_voucher_amount,
            "escrowContract": cs.escrow_contract,
            "status": STATUS_CLOSED,
            "reference": sa_receipt.reference if sa_receipt else "",
        }

    # --- Internal helpers ---

    async def _get_channel(self, channel_id: str) -> ChannelState:
        data = await self._store.get(channel_id)
        if data is None:
            raise ChannelNotFoundError(detail=f"channel {channel_id!r} not found")
        cs = ChannelState.from_dict(data) if isinstance(data, dict) else data
        if cs.finalized:
            raise ChannelClosedError(detail=f"channel {channel_id!r} is finalized")
        return cs

    async def _sign_and_submit_close(
        self,
        channel_id: str,
        channel_id_bytes: bytes,
        cumulative_amount: int,
        voucher_signature_hex: str,
        escrow_contract: str,
        chain_id: int,
    ) -> Any:
        """Sign CloseAuthorization, submit to SA API."""
        if self._signer is None:
            raise InvalidPayloadError(detail="payee signer not configured")

        nonce = await self._nonce_provider.allocate(self._recipient, channel_id)
        deadline = self._default_deadline

        payee_sig = sign_authorization(
            signer=self._signer,
            primary_type="CloseAuthorization",
            channel_id=channel_id_bytes,
            cumulative_amount=cumulative_amount,
            nonce=nonce,
            deadline=deadline,
            escrow_contract=escrow_contract,
            chain_id=chain_id,
            domain_name=self._domain_name,
            domain_version=self._domain_version,
        )

        sa_req = SessionCloseRequest(
            payload=SessionClosePayload(
                action=ACTION_CLOSE,
                channel_id=channel_id,
                cumulative_amount=str(cumulative_amount),
                voucher_signature=voucher_signature_hex or "",
                payee_signature="0x" + payee_sig.hex(),
                nonce=str(nonce),
                deadline=str(deadline),
            )
        )
        return await self._sa_client.session_close(sa_req)

    def _get_payload_dict(self, credential: Any) -> dict:
        """Extract payload as dict from credential object."""
        if hasattr(credential, "payload"):
            p = credential.payload
            return p if isinstance(p, dict) else (p.model_dump(by_alias=True) if hasattr(p, "model_dump") else vars(p))
        return credential if isinstance(credential, dict) else {}

    def _resolve_payer(self, credential: Any, payload: dict) -> str:
        """Resolve payer address from source DID or authorization.from."""
        source = getattr(credential, "source", None) or ""
        if source:
            address, _ = _parse_did_pkh(source)
            return address

        if payload.get("type") == "transaction":
            auth = payload.get("authorization")
            if auth and isinstance(auth, dict):
                from_addr = auth.get("from", "")
                if from_addr:
                    return from_addr.lower()

        raise InvalidPayloadError(
            detail="open: cannot determine payer (source or authorization.from required)"
        )

    def _build_open_request(self, credential: Any, payload: dict) -> SessionOpenRequest:
        """Build SA SessionOpenRequest from credential payload."""
        echo = self._get_challenge_echo(credential)
        source = getattr(credential, "source", None) or ""

        auth = None
        auth_data = payload.get("authorization")
        if auth_data and isinstance(auth_data, dict):
            auth = Eip3009Authorization(
                type=auth_data.get("type", "eip-3009"),
                from_address=auth_data.get("from", ""),
                to=auth_data.get("to", ""),
                value=auth_data.get("value", ""),
                valid_after=auth_data.get("validAfter", ""),
                valid_before=auth_data.get("validBefore", ""),
                nonce=auth_data.get("nonce", ""),
                signature=auth_data.get("signature"),
            )

        return SessionOpenRequest(
            challenge=echo,
            payload=SessionOpenPayload(
                action=ACTION_OPEN,
                type=payload.get("type", "transaction"),
                channel_id=payload.get("channelId", ""),
                salt=payload.get("salt", ""),
                authorized_signer=payload.get("authorizedSigner"),
                authorization=auth,
                signature=payload.get("signature"),
                hash=payload.get("hash"),
            ),
            source=source or None,
        )

    def _build_top_up_request(self, credential: Any, payload: dict) -> SessionTopUpRequest:
        """Build SA SessionTopUpRequest from credential payload."""
        echo = self._get_challenge_echo(credential)
        source = getattr(credential, "source", None) or ""

        auth = None
        auth_data = payload.get("authorization")
        if auth_data and isinstance(auth_data, dict):
            auth = Eip3009Authorization(
                type=auth_data.get("type", "eip-3009"),
                from_address=auth_data.get("from", ""),
                to=auth_data.get("to", ""),
                value=auth_data.get("value", ""),
                valid_after=auth_data.get("validAfter", ""),
                valid_before=auth_data.get("validBefore", ""),
                nonce=auth_data.get("nonce", ""),
                signature=auth_data.get("signature"),
            )

        return SessionTopUpRequest(
            challenge=echo,
            payload=SessionTopUpPayload(
                action=ACTION_TOP_UP,
                type=payload.get("type", "transaction"),
                channel_id=payload.get("channelId", ""),
                additional_deposit=payload.get("additionalDeposit", "0"),
                authorization=auth,
                signature=payload.get("signature"),
                hash=payload.get("hash"),
                top_up_salt=payload.get("topUpSalt"),
            ),
            source=source or None,
        )

    def _get_challenge_echo(self, credential: Any) -> ChallengeEcho | None:
        """Extract challenge echo from credential if available."""
        challenge = getattr(credential, "challenge", None)
        if challenge is None:
            return None
        if isinstance(challenge, dict):
            return ChallengeEcho.model_validate(challenge)
        if hasattr(challenge, "id"):
            return ChallengeEcho(
                id=challenge.id,
                realm=getattr(challenge, "realm", ""),
                method=getattr(challenge, "method", "evm"),
                intent=getattr(challenge, "intent", "session"),
                request=getattr(challenge, "request", ""),
                expires=getattr(challenge, "expires", None),
            )
        return None
