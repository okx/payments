"""Unit tests for x402.http.okx_facilitator_client — OKX facilitator client."""

from __future__ import annotations

import json
from typing import Any

import httpx
import pytest
from httpx import Response

from x402.http.okx_auth import OKXAuthConfig
from x402.http.okx_facilitator_client import (
    OKX_BASE_PATH,
    OKXFacilitatorClient,
    OKXFacilitatorClientSync,
    OKXFacilitatorConfig,
    OKXFacilitatorResponseError,
)
from x402.schemas import PaymentPayload, PaymentRequirements


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_AUTH = OKXAuthConfig(api_key="test-key", secret_key="test-secret", passphrase="test-pass")


def _make_requirements() -> PaymentRequirements:
    return PaymentRequirements(
        scheme="exact",
        network="eip155:196",
        asset="0xUSDC",
        amount="1000000",
        pay_to="0xmerchant",
        max_timeout_seconds=30,
    )


def _make_payload() -> PaymentPayload:
    return PaymentPayload(
        x402_version=2,
        payload={"signature": "0xdead"},
        accepted=_make_requirements(),
    )


def _config_with(transport: httpx.BaseTransport, **overrides: Any) -> OKXFacilitatorConfig:
    return OKXFacilitatorConfig(
        auth=_AUTH,
        http_client=httpx.Client(transport=transport),
        **overrides,
    )


def _envelope(data: dict[str, Any], code: int = 0, msg: str = "") -> bytes:
    return json.dumps({"code": code, "msg": msg, "data": data}).encode()


# ---------------------------------------------------------------------------
# Sync client tests (mirrors Go test coverage)
# ---------------------------------------------------------------------------


class TestOKXFacilitatorClientSync:
    def test_get_supported(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                assert request.url.path == f"{OKX_BASE_PATH}/supported"
                assert request.method == "GET"
                assert request.headers.get("ok-access-key") == "test-key"
                assert request.headers.get("ok-access-sign")
                return Response(200, content=_envelope({
                    "kinds": [
                        {"x402Version": 2, "scheme": "exact", "network": "eip155:196"},
                        {"x402Version": 2, "scheme": "exact", "network": "eip155:8453"},
                    ],
                    "extensions": [],
                    "signers": {},
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        supported = client.get_supported()
        assert len(supported.kinds) == 2
        assert supported.kinds[0].network == "eip155:196"
        assert supported.kinds[1].network == "eip155:8453"
        for kind in supported.kinds:
            assert kind.x402_version == 2
            assert kind.scheme == "exact"

    def test_verify(self):
        captured: dict[str, Any] = {}

        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                assert request.url.path == f"{OKX_BASE_PATH}/verify"
                assert request.method == "POST"
                captured["body"] = json.loads(request.content)
                return Response(200, content=_envelope({"isValid": True, "payer": "0xabc123"}))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        resp = client.verify(_make_payload(), _make_requirements())

        assert resp.is_valid is True
        assert resp.payer == "0xabc123"
        assert captured["body"]["x402Version"] == 2
        assert "paymentPayload" in captured["body"]
        assert "paymentRequirements" in captured["body"]
        assert "syncSettle" not in captured["body"]

    def test_settle(self):
        captured: dict[str, Any] = {}

        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                assert request.url.path == f"{OKX_BASE_PATH}/settle"
                assert request.method == "POST"
                captured["body"] = json.loads(request.content)
                return Response(200, content=_envelope({
                    "success": True,
                    "transaction": "0xtx123",
                    "network": "eip155:196",
                    "payer": "0xpayer",
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        resp = client.settle(_make_payload(), _make_requirements())

        assert resp.success is True
        assert resp.transaction == "0xtx123"
        assert resp.network == "eip155:196"
        assert resp.payer == "0xpayer"
        assert captured["body"]["syncSettle"] is True

    def test_settle_async_mode(self):
        captured: dict[str, Any] = {}

        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                captured["body"] = json.loads(request.content)
                return Response(200, content=_envelope({
                    "success": True,
                    "transaction": "0xtxasync",
                    "network": "eip155:196",
                    "payer": "0xpayer",
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport(), sync_settle=False))
        client.settle(_make_payload(), _make_requirements())
        assert captured["body"]["syncSettle"] is False

    def test_error_envelope(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(200, content=json.dumps({
                    "code": 50103,
                    "msg": "Invalid API key",
                    "error_code": "50103",
                    "error_message": "Invalid API key",
                    "data": {},
                }).encode())

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        with pytest.raises(OKXFacilitatorResponseError, match="50103") as exc_info:
            client.get_supported()
        assert "Invalid API key" in str(exc_info.value)

    def test_missing_credentials(self):
        with pytest.raises(ValueError, match="required"):
            OKXFacilitatorClientSync(OKXFacilitatorConfig(
                auth=OKXAuthConfig(api_key="", secret_key="secret", passphrase="pass"),
            ))
        with pytest.raises(ValueError, match="required"):
            OKXFacilitatorClientSync(OKXFacilitatorConfig(
                auth=OKXAuthConfig(api_key="key", secret_key="", passphrase="pass"),
            ))
        with pytest.raises(ValueError, match="required"):
            OKXFacilitatorClientSync(OKXFacilitatorConfig(
                auth=OKXAuthConfig(api_key="key", secret_key="secret", passphrase=""),
            ))

    def test_verify_invalid_signature(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(200, content=_envelope({
                    "isValid": False,
                    "invalidReason": "signature mismatch",
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        resp = client.verify(_make_payload(), _make_requirements())
        assert resp.is_valid is False
        assert resp.invalid_reason == "signature mismatch"

    def test_settle_insufficient_balance(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(200, content=_envelope({
                    "success": False,
                    "transaction": "",
                    "network": "eip155:196",
                    "errorReason": "insufficient balance",
                    "payer": "0xpayer",
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        resp = client.settle(_make_payload(), _make_requirements())
        assert resp.success is False
        assert resp.error_reason == "insufficient balance"

    def test_http_error_status(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(500, content=b"Internal Server Error")

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        with pytest.raises(OKXFacilitatorResponseError, match="HTTP 500"):
            client.get_supported()

    def test_no_envelope_fallback(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(200, content=json.dumps({
                    "kinds": [{"x402Version": 2, "scheme": "exact", "network": "eip155:1"}],
                    "extensions": [],
                    "signers": {},
                }).encode())

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        supported = client.get_supported()
        assert len(supported.kinds) == 1

    def test_get_settle_status(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                assert "/settle/status" in str(request.url)
                assert "txHash=0xhash" in str(request.url)
                assert request.method == "GET"
                return Response(200, content=_envelope({
                    "success": True,
                    "status": "success",
                    "transaction": "0xhash",
                    "network": "eip155:196",
                }))

        client = OKXFacilitatorClientSync(_config_with(Transport()))
        resp = client.get_settle_status("0xhash")
        assert resp.success is True
        assert resp.status == "success"

    def test_context_manager(self):
        class Transport(httpx.BaseTransport):
            def handle_request(self, request: httpx.Request) -> Response:
                return Response(200, content=_envelope({
                    "kinds": [], "extensions": [], "signers": {},
                }))

        with OKXFacilitatorClientSync(_config_with(Transport())) as client:
            client.get_supported()


# ---------------------------------------------------------------------------
# Async client tests
# ---------------------------------------------------------------------------


class _AsyncTransport(httpx.AsyncBaseTransport):
    def __init__(self, handler):
        self._handler = handler

    async def handle_async_request(self, request: httpx.Request) -> Response:
        return self._handler(request)


def _async_config(handler, **overrides: Any) -> OKXFacilitatorConfig:
    return OKXFacilitatorConfig(
        auth=_AUTH,
        http_client=httpx.AsyncClient(transport=_AsyncTransport(handler)),
        **overrides,
    )


class TestOKXFacilitatorClientAsync:
    @pytest.mark.asyncio
    async def test_verify(self):
        def handler(req: httpx.Request) -> Response:
            return Response(200, content=_envelope({"isValid": True, "payer": "0xabc"}))

        async with OKXFacilitatorClient(_async_config(handler)) as client:
            resp = await client.verify(_make_payload(), _make_requirements())
            assert resp.is_valid is True
            assert resp.payer == "0xabc"

    @pytest.mark.asyncio
    async def test_settle(self):
        captured: dict[str, Any] = {}

        def handler(req: httpx.Request) -> Response:
            captured["body"] = json.loads(req.content)
            return Response(200, content=_envelope({
                "success": True, "transaction": "0xtx", "network": "eip155:196",
            }))

        async with OKXFacilitatorClient(_async_config(handler)) as client:
            resp = await client.settle(_make_payload(), _make_requirements())
            assert resp.success is True
            assert captured["body"]["syncSettle"] is True

    @pytest.mark.asyncio
    async def test_get_settle_status(self):
        def handler(req: httpx.Request) -> Response:
            return Response(200, content=_envelope({
                "success": True, "status": "success", "transaction": "0xh",
            }))

        async with OKXFacilitatorClient(_async_config(handler)) as client:
            resp = await client.get_settle_status("0xh")
            assert resp.success is True
            assert resp.status == "success"

    @pytest.mark.asyncio
    async def test_error_envelope(self):
        def handler(req: httpx.Request) -> Response:
            return Response(200, content=json.dumps({"code": 99, "msg": "fail", "data": None}).encode())

        async with OKXFacilitatorClient(_async_config(handler)) as client:
            with pytest.raises(OKXFacilitatorResponseError, match="99"):
                await client.verify(_make_payload(), _make_requirements())
