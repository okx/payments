"""Tests for the pympp#139 session respond monkey-patch.

Verifies that:
- Mpp.pay(intent="session") short-circuits management actions via respond()
- Voucher actions still reach the handler
- Charge intents (no respond) are unaffected
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from typing import Any
from unittest.mock import AsyncMock, MagicMock

import pytest
from starlette.requests import Request
from starlette.testclient import TestClient

# Import mpp_evm to trigger the patch
import mpp_evm  # noqa: F401
from mpp import Challenge, Credential, ChallengeEcho, Receipt
from mpp.server.mpp import Mpp

from mpp_evm._defaults import ACTION_OPEN, ACTION_VOUCHER


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


class FakeChargeIntent:
    """Charge intent with no respond method."""

    name = "charge"

    async def verify(self, credential: Credential, request: dict) -> Receipt:
        return Receipt.success(reference="0xcharge_ref", method="evm")


class FakeSessionIntent:
    """Session intent with respond method — mirrors SessionIntent."""

    name = "session"

    async def verify(self, credential: Credential, request: dict) -> Receipt:
        return Receipt.success(reference="0xsession_ref", method="evm")

    def respond(self, credential: Credential, receipt: Any) -> Any:
        payload = credential.payload
        action = payload.get("action", "") if isinstance(payload, dict) else ""
        if action == ACTION_VOUCHER:
            return None  # proceed to handler
        return {"status": "ok", "receipt": {"reference": receipt.reference}}


class FakeMethod:
    name = "evm"
    currency = "0xtoken"
    recipient = "0xrecipient"
    decimals = 6
    chain_id = 196

    def __init__(self, intents: dict) -> None:
        self._intents = intents

    @property
    def intents(self) -> dict:
        return self._intents


@pytest.fixture
def session_method() -> FakeMethod:
    return FakeMethod(
        intents={
            "charge": FakeChargeIntent(),
            "session": FakeSessionIntent(),
        }
    )


@pytest.fixture
def mpp_server(session_method) -> Mpp:
    return Mpp(
        method=session_method,
        realm="test.example.com",
        secret_key="test-secret-key",
    )


# ---------------------------------------------------------------------------
# Tests: patch applied
# ---------------------------------------------------------------------------


class TestPatchApplied:
    def test_mpp_pay_is_patched(self) -> None:
        assert getattr(Mpp.pay, "_session_respond_patched", False) is True

    def test_wrap_payment_handler_has_respond_fn(self) -> None:
        from mpp.server.decorator import wrap_payment_handler

        assert "respond_fn" in wrap_payment_handler.__code__.co_varnames


# ---------------------------------------------------------------------------
# Tests: session respond behavior
# ---------------------------------------------------------------------------


class TestSessionRespond:
    @pytest.mark.asyncio
    async def test_session_management_action_short_circuits(self, mpp_server) -> None:
        """Open action should be intercepted by respond(), not reach handler."""
        handler_called = False

        @mpp_server.pay(amount="0.00001", intent="session")
        async def handler(request, credential, receipt):
            nonlocal handler_called
            handler_called = True
            return {"data": "should not reach"}

        # Build a valid credential with open action
        expires = (datetime.now(UTC) + timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
        challenge = Challenge.create(
            secret_key="test-secret-key",
            realm="test.example.com",
            expires=expires,
            method="evm",
            intent="session",
            request={
                "amount": "10",
                "currency": "0xtoken",
                "recipient": "0xrecipient",
                "methodDetails": {"chainId": 196},
            },
        )
        echo = challenge.to_echo()
        credential = Credential(
            challenge=echo,
            payload={"action": "open", "channelId": "0xabc"},
        )
        auth_header = credential.to_authorization()

        # Simulate request with Authorization header
        mock_request = MagicMock()
        mock_request.headers = {"authorization": auth_header}

        result = await handler(mock_request)

        assert handler_called is False
        # Result should be a JSONResponse with management response
        assert hasattr(result, "status_code")
        assert result.status_code == 200

    @pytest.mark.asyncio
    async def test_session_voucher_reaches_handler(self, mpp_server) -> None:
        """Voucher action should pass through respond() to handler."""
        handler_called = False

        @mpp_server.pay(amount="0.00001", intent="session")
        async def handler(request, credential, receipt):
            nonlocal handler_called
            handler_called = True
            return {"data": "served!"}

        expires = (datetime.now(UTC) + timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
        challenge = Challenge.create(
            secret_key="test-secret-key",
            realm="test.example.com",
            expires=expires,
            method="evm",
            intent="session",
            request={
                "amount": "10",
                "currency": "0xtoken",
                "recipient": "0xrecipient",
                "methodDetails": {"chainId": 196},
            },
        )
        echo = challenge.to_echo()
        credential = Credential(
            challenge=echo,
            payload={"action": "voucher", "channelId": "0xabc", "cumulativeAmount": "100"},
        )
        auth_header = credential.to_authorization()

        mock_request = MagicMock()
        mock_request.headers = {"authorization": auth_header}

        result = await handler(mock_request)

        assert handler_called is True
        assert result == {"data": "served!"}

    @pytest.mark.asyncio
    async def test_no_auth_returns_challenge(self, mpp_server) -> None:
        """Missing Authorization header should return 402 challenge."""

        @mpp_server.pay(amount="0.00001", intent="session")
        async def handler(request, credential, receipt):
            return {"data": "should not reach"}

        mock_request = MagicMock()
        mock_request.headers = {}

        result = await handler(mock_request)

        # Should be a Starlette Response with 402
        assert hasattr(result, "status_code")
        assert result.status_code == 402

    @pytest.mark.asyncio
    async def test_charge_intent_unaffected(self, mpp_server) -> None:
        """Charge intent (no respond) should work as before."""
        handler_called = False

        @mpp_server.pay(amount="0.01", intent="charge")
        async def handler(request, credential, receipt):
            nonlocal handler_called
            handler_called = True
            return {"data": "charged!"}

        expires = (datetime.now(UTC) + timedelta(minutes=5)).isoformat().replace("+00:00", "Z")
        challenge = Challenge.create(
            secret_key="test-secret-key",
            realm="test.example.com",
            expires=expires,
            method="evm",
            intent="charge",
            request={
                "amount": "10000",
                "currency": "0xtoken",
                "recipient": "0xrecipient",
                "methodDetails": {"chainId": 196},
            },
        )
        echo = challenge.to_echo()
        credential = Credential(
            challenge=echo,
            payload={"type": "transaction", "authorization": {}},
        )
        auth_header = credential.to_authorization()

        mock_request = MagicMock()
        mock_request.headers = {"authorization": auth_header}

        result = await handler(mock_request)

        assert handler_called is True
        assert result == {"data": "charged!"}
