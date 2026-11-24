"""Tests for paymentrouter.fastapi.middleware — PaymentGate and PaymentMiddleware."""

from __future__ import annotations

import pytest
from fastapi import Depends, FastAPI
from httpx import ASGITransport, AsyncClient
from starlette.requests import Request

from paymentrouter.fastapi.middleware import (
    PaymentGate,
    PaymentMiddleware,
    register_management_handler,
)

from .conftest import MockAdapter


def _build_middleware_app(gate: PaymentGate, routes: dict) -> FastAPI:
    """Build a minimal FastAPI app with PaymentMiddleware."""
    app = FastAPI()

    @app.get("/api/weather")
    async def weather() -> dict:
        return {"temp": 72}

    @app.get("/free")
    async def free() -> dict:
        return {"free": True}

    app.add_middleware(PaymentMiddleware, gate=gate, routes=routes)
    return app


class TestForRoute:
    @pytest.mark.asyncio
    async def test_for_route_returns_402_when_no_payment(self) -> None:
        """Request without payment headers gets 402 with challenge headers."""
        adapter = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=False,
            challenge_result={"X-Payment": "required"},
        )
        gate = PaymentGate([adapter])
        cfg = {"mpp": {}}

        app = FastAPI()
        dep = gate.for_route(cfg)

        @app.get("/resource", dependencies=[Depends(dep)])
        async def handler() -> dict:
            return {"ok": True}

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/resource")

        assert resp.status_code == 402
        assert "x-payment" in resp.headers

    @pytest.mark.asyncio
    async def test_for_route_multiple_challenges_separate_header_lines(self) -> None:
        """Two adapters challenging the same header → separate lines, not folded."""
        a1 = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=False,
            challenge_result={"WWW-Authenticate": "MPP realm=mpp"},
        )
        a2 = MockAdapter(
            name="x402",
            priority=2,
            detect_result=False,
            challenge_result={"WWW-Authenticate": "X402 realm=x402"},
        )
        gate = PaymentGate([a1, a2])
        cfg = {"mpp": {}, "x402": {}}

        app = FastAPI()
        register_management_handler(app)  # required for the multi-header challenge path
        dep = gate.for_route(cfg)

        @app.get("/resource", dependencies=[Depends(dep)])
        async def handler() -> dict:
            return {"ok": True}

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/resource")

        assert resp.status_code == 402
        assert resp.headers.get_list("www-authenticate") == [
            "MPP realm=mpp",
            "X402 realm=x402",
        ]

    @pytest.mark.asyncio
    async def test_for_route_passes_when_payment_detected(self) -> None:
        """Mock adapter detects and handles, handler returns 200."""
        adapter = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=True,
            handle_result={"paid": True},
        )
        gate = PaymentGate([adapter])
        cfg = {"mpp": {}}

        app = FastAPI()
        dep = gate.for_route(cfg)

        @app.get("/resource", dependencies=[Depends(dep)])
        async def handler() -> dict:
            return {"ok": True}

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/resource")

        assert resp.status_code == 200
        assert resp.json() == {"ok": True}

    @pytest.mark.asyncio
    async def test_for_route_handle_error_calls_on_error(self) -> None:
        """adapter.handle raises, on_error called, returns error response."""
        adapter = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=True,
            handle_result=RuntimeError("payment failed"),
        )

        errors: list[tuple[Exception, str, str]] = []

        def _on_error(exc: Exception, phase: str, adapter_name: str) -> None:
            errors.append((exc, phase, adapter_name))

        gate = PaymentGate([adapter], on_error=_on_error)
        cfg = {"mpp": {}}

        app = FastAPI()
        dep = gate.for_route(cfg)

        @app.get("/resource", dependencies=[Depends(dep)])
        async def handler() -> dict:
            return {"ok": True}

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/resource")

        assert resp.status_code == 400
        assert len(errors) == 1
        assert errors[0][1] == "handle"
        assert errors[0][2] == "mpp"


class TestPaymentMiddleware:
    @pytest.mark.asyncio
    async def test_middleware_matches_route_and_returns_402(self) -> None:
        """PaymentMiddleware with route map, unauthed request gets 402."""
        adapter = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=False,
            challenge_result={"X-Challenge": "pay-me"},
        )
        gate = PaymentGate([adapter])
        routes = {"GET /api/weather": {"mpp": {}}}

        app = _build_middleware_app(gate, routes)

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/api/weather")

        assert resp.status_code == 402
        assert "x-challenge" in resp.headers

    @pytest.mark.asyncio
    async def test_middleware_passes_unmatched_routes(self) -> None:
        """Request to non-payment route passes through."""
        adapter = MockAdapter(name="mpp", priority=1, detect_result=False)
        gate = PaymentGate([adapter])
        routes = {"GET /api/weather": {"mpp": {}}}

        app = _build_middleware_app(gate, routes)

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/free")

        assert resp.status_code == 200
        assert resp.json() == {"free": True}

    @pytest.mark.asyncio
    async def test_middleware_multiple_adapters_merge_challenges(self) -> None:
        """Two adapters, merged challenge headers in 402."""
        a1 = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=False,
            challenge_result={"WWW-Authenticate": "MPP realm=mpp"},
        )
        a2 = MockAdapter(
            name="x402",
            priority=2,
            detect_result=False,
            challenge_result={"WWW-Authenticate": "X402 realm=x402"},
        )
        gate = PaymentGate([a1, a2])
        routes = {"GET /api/weather": {"mpp": {}, "x402": {}}}

        app = _build_middleware_app(gate, routes)

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/api/weather")

        assert resp.status_code == 402
        # Each challenge MUST be its own header line, not comma-folded into one
        # (RFC 7235; mirrors Go Header().Add / Rust append). httpx exposes the
        # separate lines via get_list.
        challenges = resp.headers.get_list("www-authenticate")
        assert challenges == ["MPP realm=mpp", "X402 realm=x402"]

    @pytest.mark.asyncio
    async def test_middleware_management_credential_does_not_serve_resource(self) -> None:
        """A session-management credential must NOT pass through to the protected resource.

        Regression for the middleware payment-bypass: management results
        (open/topUp/close/settle, marked _management=True) must short-circuit
        with the management response — never serve the gated handler — matching
        the Depends/_run_gate path.
        """
        mgmt_response = {"status": "open", "channelId": "0xabc"}
        adapter = MockAdapter(
            name="mpp",
            priority=1,
            detect_result=True,  # a credential is present on the request
            handle_result={"_management": True, "response": mgmt_response},
        )
        gate = PaymentGate([adapter])
        routes = {"GET /api/weather": {"mpp": {}}}

        app = _build_middleware_app(gate, routes)

        transport = ASGITransport(app=app)
        async with AsyncClient(transport=transport, base_url="http://test") as client:
            resp = await client.get("/api/weather")

        # Must return the management response, NOT the protected handler body.
        assert resp.status_code == 200
        assert resp.json() == mgmt_response
        assert resp.json() != {"temp": 72}
