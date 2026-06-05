"""Tests for EVM charge intent — settle and verifyHash flows."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any
from unittest.mock import AsyncMock

import pytest

from mpp_evm.charge.intent import ChargeIntent
from mpp_evm.charge.method import EvmChargeMethod
from mpp_evm.charge.schemas import (
    ChargeMethodDetails,
    ChargeRequest,
    ChargeTransactionPayload,
    ChargeHashPayload,
    Eip3009Authorization,
)
from mpp_evm.errors import (
    ChannelNotFoundError,
    InternalSAError,
    InvalidSignatureError,
    MalformedCredentialError,
)
from mpp_evm.method import EvmMethod
from mpp_evm.saclient.types import ChargeReceipt as SAChargeReceipt


# ---------------------------------------------------------------------------
# Test fixtures
# ---------------------------------------------------------------------------


@dataclass(frozen=True, slots=True)
class FakeChallengeEcho:
    id: str = "ch-test-123"
    realm: str = "test.example.com"
    method: str = "evm"
    intent: str = "charge"
    request: str = "base64data"
    expires: str | None = "2026-12-31T23:59:59Z"


@dataclass(frozen=True, slots=True)
class FakeCredential:
    challenge: FakeChallengeEcho
    payload: dict[str, Any]
    source: str | None = None


@pytest.fixture
def mock_sa_client() -> AsyncMock:
    """Mock SA client with default successful responses."""
    client = AsyncMock()
    client.settle = AsyncMock(
        return_value=SAChargeReceipt(
            method="evm",
            reference="0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            status="success",
            timestamp="2026-05-04T12:00:00Z",
            chain_id=196,
            challenge_id="ch-test-123",
            external_id="order-456",
        )
    )
    client.verify_hash = AsyncMock(
        return_value=SAChargeReceipt(
            method="evm",
            reference="0x1111111111111111111111111111111111111111111111111111111111111111",
            status="success",
            timestamp="2026-05-04T12:00:00Z",
            chain_id=196,
            challenge_id="ch-test-123",
            external_id="order-789",
        )
    )
    return client


@pytest.fixture
def charge_intent(mock_sa_client: AsyncMock) -> ChargeIntent:
    return ChargeIntent(
        sa_client=mock_sa_client,
        chain_id=196,
        recipient="0x742d35Cc6634c0532925a3b844bC9e7595F8fE00",
        fee_payer=True,
    )


@pytest.fixture
def transaction_credential() -> FakeCredential:
    return FakeCredential(
        challenge=FakeChallengeEcho(),
        payload={
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0x1234567890abcdef1234567890abcdef12345678",
                "to": "0x742d35Cc6634c0532925a3b844bC9e7595F8fE00",
                "value": "1000000",
                "validAfter": "0",
                "validBefore": "1775059500",
                "nonce": "0x6d0f1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab",
                "signature": "0x" + "ab" * 65,
            },
        },
        source="did:pkh:eip155:196:0x1234567890abcdef1234567890abcdef12345678",
    )


@pytest.fixture
def hash_credential() -> FakeCredential:
    return FakeCredential(
        challenge=FakeChallengeEcho(),
        payload={
            "type": "hash",
            "hash": "0x1111111111111111111111111111111111111111111111111111111111111111",
        },
        source="did:pkh:eip155:196:0x1234567890abcdef1234567890abcdef12345678",
    )


@pytest.fixture
def charge_request() -> dict[str, Any]:
    return {
        "amount": "1000000",
        "currency": "0xA8CE8aee21bC2A48a5EF670afCc9274C7bbbC035",
        "recipient": "0x742d35Cc6634c0532925a3b844bC9e7595F8fE00",
        "description": "API access",
        "methodDetails": {"chainId": 196, "feePayer": True},
    }


# ---------------------------------------------------------------------------
# ChargeIntent.verify() tests — transaction mode
# ---------------------------------------------------------------------------


class TestChargeIntentTransaction:
    """Tests for ChargeIntent.verify() with transaction payload."""

    @pytest.mark.asyncio
    async def test_settle_success(
        self,
        charge_intent: ChargeIntent,
        transaction_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        receipt = await charge_intent.verify(transaction_credential, charge_request)
        assert receipt.reference == "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        assert receipt.method == "evm"
        mock_sa_client.settle.assert_called_once()

    @pytest.mark.asyncio
    async def test_settle_passes_challenge_echo(
        self,
        charge_intent: ChargeIntent,
        transaction_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        await charge_intent.verify(transaction_credential, charge_request)
        call_args = mock_sa_client.settle.call_args[0][0]
        assert call_args.challenge is not None
        assert call_args.challenge.id == "ch-test-123"
        assert call_args.challenge.realm == "test.example.com"
        assert call_args.challenge.method == "evm"
        assert call_args.challenge.intent == "charge"

    @pytest.mark.asyncio
    async def test_settle_passes_source(
        self,
        charge_intent: ChargeIntent,
        transaction_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        await charge_intent.verify(transaction_credential, charge_request)
        call_args = mock_sa_client.settle.call_args[0][0]
        assert call_args.source == "did:pkh:eip155:196:0x1234567890abcdef1234567890abcdef12345678"

    @pytest.mark.asyncio
    async def test_settle_passes_authorization_fields(
        self,
        charge_intent: ChargeIntent,
        transaction_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        await charge_intent.verify(transaction_credential, charge_request)
        call_args = mock_sa_client.settle.call_args[0][0]
        auth = call_args.payload.authorization
        assert auth.from_address == "0x1234567890abcdef1234567890abcdef12345678"
        assert auth.to == "0x742d35Cc6634c0532925a3b844bC9e7595F8fE00"
        assert auth.value == "1000000"
        assert auth.valid_after == "0"
        assert auth.valid_before == "1775059500"

    @pytest.mark.asyncio
    async def test_settle_with_splits(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={
                "type": "transaction",
                "authorization": {
                    "type": "eip-3009",
                    "from": "0xpayer",
                    "to": "0xrecipient",
                    "value": "950000",
                    "validAfter": "0",
                    "validBefore": "99999",
                    "nonce": "0xnonce",
                    "signature": "0xsig",
                    "splits": [
                        {
                            "type": "eip-3009",
                            "from": "0xpayer",
                            "to": "0xplatform",
                            "value": "50000",
                            "validAfter": "0",
                            "validBefore": "99999",
                            "nonce": "0xnonce2",
                            "signature": "0xsig2",
                        }
                    ],
                },
            },
            source="did:pkh:eip155:196:0xpayer",
        )
        receipt = await charge_intent.verify(cred, charge_request)
        assert receipt.reference != ""
        call_args = mock_sa_client.settle.call_args[0][0]
        assert call_args.payload.authorization.splits is not None
        assert len(call_args.payload.authorization.splits) == 1
        assert call_args.payload.authorization.splits[0].to == "0xplatform"
        assert call_args.payload.authorization.splits[0].value == "50000"

    @pytest.mark.asyncio
    async def test_settle_missing_authorization_raises(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={"type": "transaction"},
        )
        from mpp.errors import VerificationError

        with pytest.raises(VerificationError, match="authorization"):
            await charge_intent.verify(cred, charge_request)

    @pytest.mark.asyncio
    async def test_settle_sa_error_propagates(
        self,
        charge_intent: ChargeIntent,
        transaction_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        mock_sa_client.settle.side_effect = InvalidSignatureError(detail="bad sig")
        with pytest.raises(InvalidSignatureError):
            await charge_intent.verify(transaction_credential, charge_request)


# ---------------------------------------------------------------------------
# ChargeIntent.verify() tests — hash mode
# ---------------------------------------------------------------------------


class TestChargeIntentHash:
    """Tests for ChargeIntent.verify() with hash payload."""

    @pytest.mark.asyncio
    async def test_verify_hash_success(
        self,
        charge_intent: ChargeIntent,
        hash_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        receipt = await charge_intent.verify(hash_credential, charge_request)
        assert receipt.reference == "0x1111111111111111111111111111111111111111111111111111111111111111"
        assert receipt.method == "evm"
        mock_sa_client.verify_hash.assert_called_once()

    @pytest.mark.asyncio
    async def test_verify_hash_passes_hash_field(
        self,
        charge_intent: ChargeIntent,
        hash_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        await charge_intent.verify(hash_credential, charge_request)
        call_args = mock_sa_client.verify_hash.call_args[0][0]
        assert call_args.payload.hash == "0x1111111111111111111111111111111111111111111111111111111111111111"
        assert call_args.payload.type == "hash"

    @pytest.mark.asyncio
    async def test_verify_hash_missing_hash_raises(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={"type": "hash"},
        )
        from mpp.errors import VerificationError

        with pytest.raises(VerificationError, match="hash"):
            await charge_intent.verify(cred, charge_request)

    @pytest.mark.asyncio
    async def test_verify_hash_sa_error_propagates(
        self,
        charge_intent: ChargeIntent,
        hash_credential: FakeCredential,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        mock_sa_client.verify_hash.side_effect = MalformedCredentialError(detail="bad cred")
        with pytest.raises(MalformedCredentialError):
            await charge_intent.verify(hash_credential, charge_request)


# ---------------------------------------------------------------------------
# ChargeIntent.verify() tests — edge cases
# ---------------------------------------------------------------------------


class TestChargeIntentEdgeCases:
    """Tests for error handling and edge cases."""

    @pytest.mark.asyncio
    async def test_unsupported_payload_type_raises(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={"type": "unsupported"},
        )
        from mpp.errors import VerificationError

        with pytest.raises(VerificationError, match="Unsupported"):
            await charge_intent.verify(cred, charge_request)

    @pytest.mark.asyncio
    async def test_missing_type_raises(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={},
        )
        from mpp.errors import VerificationError

        with pytest.raises(VerificationError, match="Unsupported.*None"):
            await charge_intent.verify(cred, charge_request)

    @pytest.mark.asyncio
    async def test_non_dict_payload_raises(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload="not a dict",  # type: ignore
        )
        from mpp.errors import VerificationError

        with pytest.raises(VerificationError, match="not a dict"):
            await charge_intent.verify(cred, charge_request)

    @pytest.mark.asyncio
    async def test_no_source_still_works(
        self,
        charge_intent: ChargeIntent,
        charge_request: dict,
        mock_sa_client: AsyncMock,
    ) -> None:
        cred = FakeCredential(
            challenge=FakeChallengeEcho(),
            payload={
                "type": "hash",
                "hash": "0xdeadbeef",
            },
            source=None,
        )
        receipt = await charge_intent.verify(cred, charge_request)
        assert receipt.reference != ""
        call_args = mock_sa_client.verify_hash.call_args[0][0]
        assert call_args.source == ""


# ---------------------------------------------------------------------------
# EvmChargeMethod tests
# ---------------------------------------------------------------------------


class TestEvmChargeMethod:
    """Tests for EvmChargeMethod (pympp Method protocol)."""

    def test_method_name(self, mock_sa_client: AsyncMock) -> None:
        method = EvmChargeMethod(sa_client=mock_sa_client)
        assert method.name == "evm"

    def test_intents_contains_charge(self, mock_sa_client: AsyncMock) -> None:
        method = EvmChargeMethod(sa_client=mock_sa_client, chain_id=196)
        assert "charge" in method.intents
        assert isinstance(method.intents["charge"], ChargeIntent)

    def test_challenge_method_details(self, mock_sa_client: AsyncMock) -> None:
        method = EvmChargeMethod(
            sa_client=mock_sa_client, chain_id=196, fee_payer=True
        )
        details = method.challenge_method_details()
        assert details["chainId"] == 196
        assert details["feePayer"] is True

    def test_challenge_method_details_no_fee_payer(self, mock_sa_client: AsyncMock) -> None:
        method = EvmChargeMethod(sa_client=mock_sa_client, chain_id=196, fee_payer=False)
        details = method.challenge_method_details()
        assert details["chainId"] == 196
        assert details["feePayer"] is False

    @pytest.mark.asyncio
    async def test_create_credential_raises(self, mock_sa_client: AsyncMock) -> None:
        method = EvmChargeMethod(sa_client=mock_sa_client)
        with pytest.raises(NotImplementedError):
            await method.create_credential(None)  # type: ignore


# ---------------------------------------------------------------------------
# EvmMethod tests
# ---------------------------------------------------------------------------


class TestEvmMethod:
    """Tests for top-level EvmMethod."""

    def test_method_name(self) -> None:
        method = EvmMethod(intents={})
        assert method.name == "evm"

    def test_intents_property(self, mock_sa_client: AsyncMock) -> None:
        intent = ChargeIntent(sa_client=mock_sa_client)
        method = EvmMethod(intents={"charge": intent})
        assert "charge" in method.intents
        assert method.intents["charge"] is intent

    @pytest.mark.asyncio
    async def test_create_credential_raises(self) -> None:
        method = EvmMethod(intents={})
        with pytest.raises(NotImplementedError):
            await method.create_credential(None)  # type: ignore


# ---------------------------------------------------------------------------
# Schemas tests
# ---------------------------------------------------------------------------


class TestChargeSchemas:
    """Tests for charge-specific Pydantic schemas."""

    def test_charge_request_parse(self) -> None:
        data = {
            "amount": "1000000",
            "currency": "0xtoken",
            "recipient": "0xrecipient",
            "externalId": "order-1",
            "methodDetails": {"chainId": 196, "feePayer": True, "memo": "0xmemo"},
        }
        req = ChargeRequest.model_validate(data)
        assert req.amount == "1000000"
        assert req.external_id == "order-1"
        assert req.method_details is not None
        assert req.method_details.is_fee_payer is True
        assert req.method_details.memo == "0xmemo"

    def test_charge_method_details_with_splits(self) -> None:
        data = {
            "chainId": 196,
            "splits": [
                {"amount": "50000", "recipient": "0xfee", "memo": "platform"},
                {"amount": "10000", "recipient": "0xref"},
            ],
        }
        details = ChargeMethodDetails.model_validate(data)
        assert len(details.splits) == 2
        assert details.splits[0].amount == "50000"
        assert details.splits[1].memo is None

    def test_eip3009_authorization_serialization(self) -> None:
        auth = Eip3009Authorization(
            type="eip-3009",
            from_address="0xfrom",
            to="0xto",
            value="1000000",
            valid_after="0",
            valid_before="99999",
            nonce="0xnonce",
            signature="0xsig",
        )
        data = auth.model_dump(by_alias=True, exclude_none=True)
        assert data["from"] == "0xfrom"
        assert "from_address" not in data

    def test_charge_transaction_payload(self) -> None:
        payload = ChargeTransactionPayload(
            authorization=Eip3009Authorization(
                from_address="0xfrom",
                to="0xto",
                value="100",
                valid_after="0",
                valid_before="99",
                nonce="0xn",
            )
        )
        assert payload.type == "transaction"
        assert payload.authorization.from_address == "0xfrom"

    def test_charge_hash_payload(self) -> None:
        payload = ChargeHashPayload(hash="0xdeadbeef")
        assert payload.type == "hash"
        assert payload.hash == "0xdeadbeef"
