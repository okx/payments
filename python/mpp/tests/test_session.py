"""Tests for EVM session intent: voucher verification, channel lifecycle, Respond pattern."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any
from unittest.mock import AsyncMock, MagicMock

import pytest

from mpp_evm._defaults import ACTION_CLOSE, ACTION_OPEN, ACTION_SETTLE, ACTION_TOP_UP, ACTION_VOUCHER
from mpp_evm.errors import (
    AmountExceedsDepositError,
    ChannelClosedError,
    ChannelNotFoundError,
    DeltaTooSmallError,
    InsufficientBalanceError,
    InvalidPayloadError,
    InvalidSignatureError,
)
from mpp_evm.nonce import UuidNonceProvider
from mpp_evm.session.channel import ChannelState, deduct_from_channel
from mpp_evm.session.intent import SessionIntent
from mpp_evm.session.voucher import (
    SECP256K1_HALF_N,
    compute_domain_separator,
    compute_voucher_struct_hash,
    sign_authorization,
    sign_voucher,
    validate_voucher_signature,
    verify_voucher,
    voucher_signing_hash,
)
from mpp_evm.signer import PrivateKeySigner


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

# Well-known test private key (DO NOT use in production)
TEST_PRIVATE_KEY = "0x" + "ab" * 32


@pytest.fixture
def test_signer() -> PrivateKeySigner:
    return PrivateKeySigner.from_hex(TEST_PRIVATE_KEY)


@pytest.fixture
def memory_store() -> dict:
    """Simple dict-based async store for testing."""
    storage: dict[str, Any] = {}

    class _Store:
        async def get(self, key: str) -> Any | None:
            return storage.get(key)

        async def put(self, key: str, value: Any) -> None:
            storage[key] = value

        async def delete(self, key: str) -> None:
            storage.pop(key, None)

    return _Store()


@pytest.fixture
def mock_sa_client() -> AsyncMock:
    """Mock SA client that returns successful receipts."""
    client = AsyncMock()
    client.session_open.return_value = MagicMock(
        reference="0xtxhash_open", channel_id="0x" + "aa" * 32, deposit="10000000"
    )
    client.session_top_up.return_value = MagicMock(reference="0xtxhash_topup")
    client.session_settle.return_value = MagicMock(reference="0xtxhash_settle")
    client.session_close.return_value = MagicMock(reference="0xtxhash_close")
    return client


@pytest.fixture
def session_intent(mock_sa_client, memory_store, test_signer) -> SessionIntent:
    return SessionIntent(
        sa_client=mock_sa_client,
        recipient="0x742d35cc6634c0532925a3b844bc9e7595f8fe00",
        signer=test_signer,
        store=memory_store,
        chain_id=196,
        escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
        per_request_cost=10,
        min_voucher_delta=30,
    )


# ---------------------------------------------------------------------------
# Voucher Signature Tests
# ---------------------------------------------------------------------------


class TestVoucherSignature:
    """Tests for EIP-712 voucher signing and verification."""

    def test_sign_and_verify_voucher(self, test_signer: PrivateKeySigner) -> None:
        channel_id = b"\x01" * 32
        cumulative_amount = 1000000
        escrow = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b"
        chain_id = 196

        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id,
            cumulative_amount=cumulative_amount,
            escrow_contract=escrow,
            chain_id=chain_id,
        )

        assert len(sig) == 65
        assert sig[64] in (27, 28)

        result = verify_voucher(
            escrow_contract=escrow,
            chain_id=chain_id,
            channel_id=channel_id,
            cumulative_amount=cumulative_amount,
            signature=sig,
            expected_signer=test_signer.address,
        )
        assert result is True

    def test_verify_wrong_signer(self, test_signer: PrivateKeySigner) -> None:
        channel_id = b"\x02" * 32
        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id,
            cumulative_amount=500,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        result = verify_voucher(
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
            channel_id=channel_id,
            cumulative_amount=500,
            signature=sig,
            expected_signer="0x0000000000000000000000000000000000000001",
        )
        assert result is False

    def test_verify_wrong_amount(self, test_signer: PrivateKeySigner) -> None:
        channel_id = b"\x03" * 32
        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id,
            cumulative_amount=100,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        result = verify_voucher(
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
            channel_id=channel_id,
            cumulative_amount=200,  # Different amount
            signature=sig,
            expected_signer=test_signer.address,
        )
        assert result is False

    def test_validate_signature_length(self) -> None:
        with pytest.raises(InvalidSignatureError, match="65 bytes"):
            validate_voucher_signature(b"\x00" * 64)

    def test_validate_signature_high_s(self) -> None:
        sig = b"\x00" * 32 + (SECP256K1_HALF_N + 1).to_bytes(32, "big") + b"\x1b"
        with pytest.raises(InvalidSignatureError, match="high-s"):
            validate_voucher_signature(sig)

    def test_validate_signature_valid(self, test_signer: PrivateKeySigner) -> None:
        sig = sign_voucher(
            signer=test_signer,
            channel_id=b"\x04" * 32,
            cumulative_amount=0,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        validate_voucher_signature(sig)  # Should not raise

    def test_domain_separator_deterministic(self) -> None:
        sep1 = compute_domain_separator("EVM Payment Channel", "1", 196, "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b")
        sep2 = compute_domain_separator("EVM Payment Channel", "1", 196, "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b")
        assert sep1 == sep2
        assert len(sep1) == 32

    def test_different_chain_different_domain(self) -> None:
        sep1 = compute_domain_separator("EVM Payment Channel", "1", 196, "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b")
        sep2 = compute_domain_separator("EVM Payment Channel", "1", 1, "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b")
        assert sep1 != sep2


class TestAuthorizationSigning:
    """Tests for SettleAuthorization and CloseAuthorization signing."""

    def test_sign_settle_authorization(self, test_signer: PrivateKeySigner) -> None:
        sig = sign_authorization(
            signer=test_signer,
            primary_type="SettleAuthorization",
            channel_id=b"\x01" * 32,
            cumulative_amount=500000,
            nonce=12345,
            deadline=99999999,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        assert len(sig) == 65
        assert sig[64] in (27, 28)

    def test_sign_close_authorization(self, test_signer: PrivateKeySigner) -> None:
        sig = sign_authorization(
            signer=test_signer,
            primary_type="CloseAuthorization",
            channel_id=b"\x02" * 32,
            cumulative_amount=1000000,
            nonce=67890,
            deadline=2**256 - 1,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        assert len(sig) == 65

    def test_settle_and_close_produce_different_sigs(self, test_signer: PrivateKeySigner) -> None:
        kwargs = dict(
            signer=test_signer,
            channel_id=b"\x01" * 32,
            cumulative_amount=500,
            nonce=1,
            deadline=999,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        settle_sig = sign_authorization(primary_type="SettleAuthorization", **kwargs)
        close_sig = sign_authorization(primary_type="CloseAuthorization", **kwargs)
        assert settle_sig != close_sig

    def test_invalid_auth_type_raises(self, test_signer: PrivateKeySigner) -> None:
        with pytest.raises(ValueError, match="invalid authorization type"):
            sign_authorization(
                signer=test_signer,
                primary_type="InvalidType",
                channel_id=b"\x01" * 32,
                cumulative_amount=100,
                nonce=1,
                deadline=999,
                escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
                chain_id=196,
            )


# ---------------------------------------------------------------------------
# Channel State Tests
# ---------------------------------------------------------------------------


class TestChannelState:
    """Tests for ChannelState dataclass and deduction logic."""

    @pytest.mark.asyncio
    async def test_deduct_from_channel_success(self, memory_store) -> None:
        cs = ChannelState(
            channel_id="0xch1",
            chain_id=196,
            escrow_contract="0xescrow",
            payer="0xpayer",
            payee="0xpayee",
            deposit="1000000",
            highest_voucher_amount="500000",
            spent="100000",
        )
        await memory_store.put("0xch1", cs.to_dict())

        result = await deduct_from_channel(memory_store, "0xch1", 50000)
        assert int(result.spent) == 150000
        assert result.units == 1

    @pytest.mark.asyncio
    async def test_deduct_insufficient_balance(self, memory_store) -> None:
        cs = ChannelState(
            channel_id="0xch2",
            chain_id=196,
            escrow_contract="0xescrow",
            payer="0xpayer",
            payee="0xpayee",
            deposit="1000000",
            highest_voucher_amount="100",
            spent="90",
        )
        await memory_store.put("0xch2", cs.to_dict())

        with pytest.raises(InsufficientBalanceError):
            await deduct_from_channel(memory_store, "0xch2", 50)

    @pytest.mark.asyncio
    async def test_deduct_from_finalized_channel(self, memory_store) -> None:
        cs = ChannelState(
            channel_id="0xch3",
            chain_id=196,
            escrow_contract="0xescrow",
            payer="0xpayer",
            payee="0xpayee",
            finalized=True,
        )
        await memory_store.put("0xch3", cs.to_dict())

        with pytest.raises(ChannelClosedError):
            await deduct_from_channel(memory_store, "0xch3", 10)

    @pytest.mark.asyncio
    async def test_deduct_from_missing_channel(self, memory_store) -> None:
        with pytest.raises(ChannelNotFoundError):
            await deduct_from_channel(memory_store, "0xmissing", 10)

    def test_channel_state_roundtrip(self) -> None:
        cs = ChannelState(
            channel_id="0xtest",
            chain_id=196,
            escrow_contract="0xescrow",
            payer="0xpayer",
            payee="0xpayee",
            deposit="5000000",
            highest_voucher_amount="250000",
            spent="100000",
            units=5,
        )
        d = cs.to_dict()
        restored = ChannelState.from_dict(d)
        assert restored.channel_id == "0xtest"
        assert restored.deposit == "5000000"
        assert restored.units == 5

    def test_available_balance(self) -> None:
        cs = ChannelState(
            channel_id="0x",
            chain_id=196,
            escrow_contract="0x",
            payer="0x",
            payee="0x",
            highest_voucher_amount="1000",
            spent="300",
        )
        assert cs.available_balance == 700


# ---------------------------------------------------------------------------
# Session Intent Lifecycle Tests
# ---------------------------------------------------------------------------


@dataclass
class FakeCredential:
    payload: dict
    source: str = ""
    challenge: Any = None


class TestSessionIntentVoucher:
    """Tests for voucher handling in SessionIntent."""

    @pytest.mark.asyncio
    async def test_voucher_verify_and_deduct(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "aa" * 32
        channel_id_bytes = bytes.fromhex("aa" * 32)

        # Pre-populate channel
        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=0,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0x742d35cc6634c0532925a3b844bc9e7595f8fe00",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="0",
            highest_voucher_signature="0x" + sig.hex(),
            min_voucher_delta="30",
        )
        await memory_store.put(channel_id, cs.to_dict())

        # Sign a new voucher with amount 1000
        new_sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=1000,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        cred = FakeCredential(payload={
            "action": "voucher",
            "channelId": channel_id,
            "cumulativeAmount": "1000",
            "signature": "0x" + new_sig.hex(),
        })

        result = await session_intent.verify(cred, {})
        assert result["status"] == "open"
        assert result["cumulativeAmount"] == "1000"

        # Verify deduction happened
        stored = await memory_store.get(channel_id)
        assert int(stored["spent"]) == 10  # per_request_cost

    @pytest.mark.asyncio
    async def test_voucher_replay_idempotent(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "bb" * 32
        channel_id_bytes = bytes.fromhex("bb" * 32)

        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=500,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        sig_hex = "0x" + sig.hex()

        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0xpayee",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="500",
            highest_voucher_signature=sig_hex,
        )
        await memory_store.put(channel_id, cs.to_dict())

        # Replay same voucher — should succeed (idempotent)
        cred = FakeCredential(payload={
            "action": "voucher",
            "channelId": channel_id,
            "cumulativeAmount": "500",
            "signature": sig_hex,
        })
        result = await session_intent.verify(cred, {})
        assert result["status"] == "open"

    @pytest.mark.asyncio
    async def test_voucher_exceeds_deposit(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "cc" * 32
        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0xpayee",
            authorized_signer=test_signer.address.lower(),
            deposit="100",
            highest_voucher_amount="0",
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(payload={
            "action": "voucher",
            "channelId": channel_id,
            "cumulativeAmount": "200",
            "signature": "0x" + ("ab" * 65),
        })
        with pytest.raises(AmountExceedsDepositError):
            await session_intent.verify(cred, {})

    @pytest.mark.asyncio
    async def test_voucher_delta_too_small(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "dd" * 32
        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0xpayee",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="100",
            min_voucher_delta="30",
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(payload={
            "action": "voucher",
            "channelId": channel_id,
            "cumulativeAmount": "110",  # delta=10, less than min_voucher_delta=30
            "signature": "0x" + ("ab" * 65),
        })
        with pytest.raises(DeltaTooSmallError):
            await session_intent.verify(cred, {})


class TestSessionIntentRespond:
    """Tests for the Respond() short-circuit pattern."""

    def test_respond_voucher_returns_none(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "voucher", "channelId": "0x1"})
        result = session_intent.respond(cred, {"some": "receipt"})
        assert result is None

    def test_respond_open_returns_management(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "open", "channelId": "0x1"})
        result = session_intent.respond(cred, {"some": "receipt"})
        assert result["status"] == "ok"
        assert result["receipt"]["intent"] == "session"
        assert result["receipt"]["method"] == "evm"
        assert result["receipt"]["status"] == "success"
        assert result["receipt"]["settlement"]
        import base64 as _b64
        import json as _j
        decoded = _j.loads(_b64.urlsafe_b64decode(result["receipt"]["settlement"] + "=="))
        assert decoded == {"some": "receipt"}

    def test_respond_topup_returns_management(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "topUp", "channelId": "0x1"})
        result = session_intent.respond(cred, {"receipt": "data"})
        assert result is not None
        assert result["status"] == "ok"

    def test_respond_close_returns_management(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "close", "channelId": "0x1"})
        result = session_intent.respond(cred, {})
        assert result is not None

    def test_respond_settle_returns_management(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "settle", "channelId": "0x1"})
        result = session_intent.respond(cred, {})
        assert result is not None


class TestSessionIntentOpen:
    """Tests for channel open flow."""

    @pytest.mark.asyncio
    async def test_open_channel(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "11" * 32
        channel_id_bytes = bytes.fromhex("11" * 32)

        # Sign initial voucher (amount=0)
        voucher_sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=0,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        cred = FakeCredential(
            payload={
                "action": "open",
                "type": "transaction",
                "channelId": channel_id,
                "salt": "0x" + "ff" * 32,
                "cumulativeAmount": "0",
                "signature": "0xdepositsig",
                "voucherSignature": "0x" + voucher_sig.hex(),
                "authorizedSigner": test_signer.address.lower(),
                "authorization": {"from": "0xpayer123", "value": "5000000"},
                "deposit": "5000000",
            },
            source="did:pkh:eip155:196:0xpayer123",
            challenge={"id": "ch-1", "realm": "test", "method": "evm", "intent": "session", "request": ""},
        )

        result = await session_intent.verify(cred, {"currency": "0xUSDC"})
        assert result["status"] == "open"
        assert result["channelId"] == channel_id
        assert result["reference"] == "0xtxhash_open"

        # Verify channel stored
        stored = await memory_store.get(channel_id)
        assert stored is not None
        assert stored["payer"] == "0xpayer123"
        assert stored["deposit"] == 5000000

    @pytest.mark.asyncio
    async def test_open_duplicate_channel_rejected(self, session_intent, memory_store) -> None:
        channel_id = "0x" + "22" * 32
        await memory_store.put(channel_id, {"channel_id": channel_id, "finalized": False})

        cred = FakeCredential(
            payload={
                "action": "open",
                "type": "hash",
                "channelId": channel_id,
                "salt": "0x00",
                "cumulativeAmount": "0",
                "signature": "0xsig",
                "hash": "0xtxhash",
            },
            source="did:pkh:eip155:196:0xpayer",
        )
        with pytest.raises(InvalidPayloadError, match="already exists"):
            await session_intent.verify(cred, {})


class TestSessionIntentClose:
    """Tests for channel close flow."""

    @pytest.mark.asyncio
    async def test_close_channel(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "33" * 32
        channel_id_bytes = bytes.fromhex("33" * 32)

        # Sign voucher for amount=500
        voucher_sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=500,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0x742d35cc6634c0532925a3b844bc9e7595f8fe00",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="500",
            highest_voucher_signature="0x" + voucher_sig.hex(),
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(payload={
            "action": "close",
            "channelId": channel_id,
            "cumulativeAmount": "500",
            "signature": "0x" + voucher_sig.hex(),
        })

        result = await session_intent.verify(cred, {})
        assert result["status"] == "closed"
        assert result["reference"] == "0xtxhash_close"

        # Channel should be deleted
        stored = await memory_store.get(channel_id)
        assert stored is None

    @pytest.mark.asyncio
    async def test_close_amount_less_than_stored_rejected(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "44" * 32
        channel_id_bytes = bytes.fromhex("44" * 32)

        sig = sign_voucher(
            signer=test_signer,
            channel_id=channel_id_bytes,
            cumulative_amount=100,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )

        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0xpayee",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="500",
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(payload={
            "action": "close",
            "channelId": channel_id,
            "cumulativeAmount": "100",  # Less than stored 500
            "signature": "0x" + sig.hex(),
        })
        with pytest.raises(DeltaTooSmallError, match="less than stored"):
            await session_intent.verify(cred, {})


class TestSessionIntentTopUp:
    """Tests for channel top-up flow."""

    @pytest.mark.asyncio
    async def test_top_up_increases_deposit(self, session_intent, memory_store) -> None:
        channel_id = "0x" + "55" * 32
        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0xpayee",
            deposit="5000000",
            highest_voucher_amount="100",
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(
            payload={
                "action": "topUp",
                "type": "transaction",
                "channelId": channel_id,
                "additionalDeposit": "3000000",
                "authorization": {"from": "0xpayer", "value": "3000000"},
                "signature": "0xsig",
                "topUpSalt": "0xsalt",
            },
            source="did:pkh:eip155:196:0xpayer",
        )
        result = await session_intent.verify(cred, {})
        assert result["status"] == "open"

        stored = await memory_store.get(channel_id)
        assert stored["deposit"] == 8000000


class TestSessionIntentSettle:
    """Tests for merchant-initiated settle."""

    @pytest.mark.asyncio
    async def test_settle_channel(self, session_intent, memory_store, test_signer) -> None:
        channel_id = "0x" + "66" * 32
        cs = ChannelState(
            channel_id=channel_id,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            payer="0xpayer",
            payee="0x742d35cc6634c0532925a3b844bc9e7595f8fe00",
            authorized_signer=test_signer.address.lower(),
            deposit="10000000",
            highest_voucher_amount="500000",
            highest_voucher_signature="0xvouchersig",
        )
        await memory_store.put(channel_id, cs.to_dict())

        cred = FakeCredential(payload={
            "action": "settle",
            "channelId": channel_id,
        })
        result = await session_intent.verify(cred, {})
        assert result["status"] == "open"
        assert result["reference"] == "0xtxhash_settle"

        # SA client should have been called with payee signature
        session_intent._sa_client.session_settle.assert_called_once()


class TestUnknownAction:
    """Test handling of unknown actions."""

    @pytest.mark.asyncio
    async def test_unknown_action_raises(self, session_intent) -> None:
        cred = FakeCredential(payload={"action": "invalid_action"})
        with pytest.raises(InvalidPayloadError, match="unknown session action"):
            await session_intent.verify(cred, {})


class TestInMemoryChannelStore:
    """Regression tests for C3: default in-memory store."""

    @pytest.mark.asyncio
    async def test_roundtrip_and_isolation(self) -> None:
        from mpp_evm.store import InMemoryChannelStore

        store = InMemoryChannelStore()
        assert await store.get("missing") is None

        value = {"channel_id": "0xabc", "deposit": 100}
        await store.put("0xabc", value)

        got = await store.get("0xabc")
        assert got == value
        # Stored value is deep-copied: mutating the original or the returned
        # copy must not corrupt stored state (matches FileStore JSON isolation).
        value["deposit"] = 999
        got["deposit"] = 777
        assert (await store.get("0xabc"))["deposit"] == 100

        assert await store.put_if_absent("0xabc", {"x": 1}) is False
        assert await store.put_if_absent("0xnew", {"x": 1}) is True

        await store.delete("0xabc")
        assert await store.get("0xabc") is None

    @pytest.mark.asyncio
    async def test_session_intent_defaults_to_inmemory_store(
        self, mock_sa_client, test_signer
    ) -> None:
        from mpp_evm.store import InMemoryChannelStore

        # Constructed WITHOUT a store — must not be None and must work end-to-end
        # rather than raising AttributeError on the first session action.
        intent = SessionIntent(
            sa_client=mock_sa_client,
            recipient="0x742d35cc6634c0532925a3b844bc9e7595f8fe00",
            signer=test_signer,
            chain_id=196,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            per_request_cost=10,
            min_voucher_delta=30,
        )
        assert isinstance(intent._store, InMemoryChannelStore)

        channel_id = "0x" + "44" * 32
        voucher_sig = sign_voucher(
            signer=test_signer,
            channel_id=bytes.fromhex("44" * 32),
            cumulative_amount=0,
            escrow_contract="0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            chain_id=196,
        )
        cred = FakeCredential(
            payload={
                "action": "open",
                "type": "transaction",
                "channelId": channel_id,
                "salt": "0x" + "ff" * 32,
                "cumulativeAmount": "0",
                "signature": "0xdepositsig",
                "voucherSignature": "0x" + voucher_sig.hex(),
                "authorizedSigner": test_signer.address.lower(),
                "authorization": {"from": "0xpayer123", "value": "5000000"},
                "deposit": "5000000",
            },
            source="did:pkh:eip155:196:0xpayer123",
            challenge={"id": "ch-1", "realm": "test", "method": "evm", "intent": "session", "request": ""},
        )

        result = await intent.verify(cred, {"currency": "0xUSDC"})
        assert result["status"] == "open"
        stored = await intent._store.get(channel_id)
        assert stored is not None and stored["payer"] == "0xpayer123"
