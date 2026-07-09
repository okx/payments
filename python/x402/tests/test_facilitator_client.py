"""Unit tests for the HTTP facilitator client's signature-only verify."""

from __future__ import annotations

import json
from typing import Any

import httpx
import pytest
from httpx import Response

from x402.http.facilitator_client import (
    FacilitatorConfig,
    HTTPFacilitatorClient,
    HTTPFacilitatorClientSync,
)
from x402.schemas import PaymentPayload, PaymentRequirements


def _payload() -> PaymentPayload:
    return PaymentPayload(
        x402_version=2,
        payload={"signature": "0xsig", "authorization": {"from": "0xReviewer"}},
        accepted=PaymentRequirements(
            scheme="exact",
            network="eip155:196",
            asset="0xUSDC",
            amount="1000000",
            pay_to="0xmerchant",
            max_timeout_seconds=300,
        ),
    )


class _RecordingTransport(httpx.BaseTransport):
    def __init__(self) -> None:
        self.path: str | None = None
        self.body: dict[str, Any] = {}

    def handle_request(self, request: httpx.Request) -> Response:
        self.path = request.url.path
        self.body = json.loads(request.content.decode())
        return Response(200, json={"isValid": True, "payer": "0xReviewer"})


def test_sync_client_posts_to_verify_signature_and_omits_requirements():
    transport = _RecordingTransport()
    client = HTTPFacilitatorClientSync(
        FacilitatorConfig(
            url="https://facilitator.test", http_client=httpx.Client(transport=transport)
        )
    )
    resp = client.verify_signature(_payload())
    assert resp.is_valid
    assert transport.path == "/verify-signature"
    assert "paymentRequirements" not in transport.body
    assert "paymentPayload" in transport.body


def test_sync_client_raises_on_error_status():
    def handler(req: httpx.Request) -> Response:
        return Response(
            400,
            json={"isValid": False, "invalidReason": "invalid_signature"},
        )

    client = HTTPFacilitatorClientSync(
        FacilitatorConfig(
            url="https://facilitator.test",
            http_client=httpx.Client(transport=httpx.MockTransport(handler)),
        )
    )
    with pytest.raises(ValueError, match="verify-signature"):
        client.verify_signature(_payload())


@pytest.mark.asyncio
async def test_async_client_verify_signature():
    transport = httpx.MockTransport(
        lambda req: Response(200, json={"isValid": True, "payer": "0xReviewer"})
    )
    client = HTTPFacilitatorClient(
        FacilitatorConfig(
            url="https://facilitator.test", http_client=httpx.AsyncClient(transport=transport)
        )
    )
    resp = await client.verify_signature(_payload())
    assert resp.is_valid
