"""OKX facilitator client for x402 payment protocol.

Provides both async (OKXFacilitatorClient) and sync (OKXFacilitatorClientSync)
implementations that talk to OKX API at /api/v6/pay/x402/*.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any

from pydantic import ValidationError

from ..schemas import (
    PaymentPayload,
    PaymentRequirements,
    SettleResponse,
    SupportedResponse,
    VerifyResponse,
)
from ..schemas.responses import SettleStatusResponse
from ..schemas.v1 import PaymentPayloadV1, PaymentRequirementsV1
from .okx_auth import OKXAuthConfig, OKXAuthProvider

OKX_DEFAULT_BASE_URL = "https://web3.okx.com"
OKX_BASE_PATH = "/api/v6/pay/x402"


@dataclass
class OKXFacilitatorConfig:
    """Configuration for OKX facilitator client."""

    auth: OKXAuthConfig
    base_url: str = OKX_DEFAULT_BASE_URL
    sync_settle: bool = True
    timeout: float = 30.0
    http_client: Any = field(default=None, repr=False)


class OKXFacilitatorResponseError(ValueError):
    """OKX facilitator returned an error envelope or malformed response."""


def _unwrap_envelope(data: Any, endpoint: str) -> dict[str, Any]:
    """Handle OKX {code, data, msg} response envelope.

    - code != 0 → raise error with code and message
    - code == 0 and data present and non-null → return unwrapped data
    - no envelope structure → return entire body (mock facilitator fallback)
    """
    if not isinstance(data, dict):
        raise OKXFacilitatorResponseError(
            f"OKX API returned non-object response from {endpoint}: {str(data)[:200]}"
        )

    code = data.get("code")

    if code is not None:
        code_int = int(code) if isinstance(code, str) else code
        if code_int != 0:
            msg = data.get("msg") or data.get("error_message") or "unknown error"
            raise OKXFacilitatorResponseError(
                f"OKX API error (code={code_int}) from {endpoint}: {msg}"
            )

        inner = data.get("data")
        if inner is not None:
            return inner

    return data


def _build_body(
    payload_dict: dict[str, Any],
    requirements_dict: dict[str, Any],
    include_sync_settle: bool,
    sync_settle: bool,
) -> dict[str, Any]:
    """Build request body for verify/settle endpoints."""
    body: dict[str, Any] = {
        "x402Version": 2,
        "paymentPayload": payload_dict,
        "paymentRequirements": requirements_dict,
    }
    if include_sync_settle:
        body["syncSettle"] = sync_settle
    return body


def _to_json_safe(obj: Any) -> Any:
    """Convert object to JSON-safe format (handles bigints)."""
    return json.loads(
        json.dumps(
            obj,
            default=lambda x: str(x) if isinstance(x, int) and x > 2**53 else x,
        )
    )


def _validate_config(config: OKXFacilitatorConfig) -> None:
    """Validate that required OKX credentials are present."""
    auth = config.auth
    if not auth.api_key or not auth.secret_key or not auth.passphrase:
        raise ValueError(
            "OKX API credentials (api_key, secret_key, passphrase) are all required"
        )


def _dump_model(model: PaymentPayload | PaymentPayloadV1 | PaymentRequirements | PaymentRequirementsV1) -> dict[str, Any]:
    """Dump a pydantic model to a JSON-safe dict."""
    return _to_json_safe(model.model_dump(by_alias=True, exclude_none=True))


# ============================================================================
# Async OKX Facilitator Client
# ============================================================================


class OKXFacilitatorClient:
    """Async OKX facilitator client satisfying the FacilitatorClient protocol.

    Communicates with OKX API at /api/v6/pay/x402/* using httpx.AsyncClient.
    """

    def __init__(self, config: OKXFacilitatorConfig) -> None:
        _validate_config(config)
        self._auth = OKXAuthProvider(config.auth)
        base_url = config.base_url.strip() if config.base_url else ""
        self._base_url = (base_url or OKX_DEFAULT_BASE_URL).rstrip("/")
        self._sync_settle = config.sync_settle
        self._timeout = config.timeout if config.timeout else 30.0
        self._http_client = config.http_client
        self._owns_client = config.http_client is None

    def _get_async_client(self) -> Any:
        if self._http_client is None:
            import httpx

            self._http_client = httpx.AsyncClient(timeout=self._timeout, follow_redirects=True)
        return self._http_client

    def _get_sync_client(self) -> Any:
        """Create a one-shot sync client for initialization calls."""
        import httpx

        return httpx.Client(timeout=self._timeout, follow_redirects=True)

    async def aclose(self) -> None:
        if self._owns_client and self._http_client:
            await self._http_client.aclose()
            self._http_client = None

    async def __aenter__(self) -> OKXFacilitatorClient:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()

    async def verify(
        self,
        payload: PaymentPayload | PaymentPayloadV1,
        requirements: PaymentRequirements | PaymentRequirementsV1,
    ) -> VerifyResponse:
        """Verify a payment via the OKX API."""
        body = _build_body(
            _dump_model(payload),
            _dump_model(requirements),
            include_sync_settle=False,
            sync_settle=False,
        )
        data = await self._do_request_async("POST", "/verify", body)
        return _parse_response(data, VerifyResponse, "verify")

    async def settle(
        self,
        payload: PaymentPayload | PaymentPayloadV1,
        requirements: PaymentRequirements | PaymentRequirementsV1,
    ) -> SettleResponse:
        """Settle a payment via the OKX API."""
        body = _build_body(
            _dump_model(payload),
            _dump_model(requirements),
            include_sync_settle=True,
            sync_settle=self._sync_settle,
        )
        data = await self._do_request_async("POST", "/settle", body)
        return _parse_response(data, SettleResponse, "settle")

    def get_supported(self) -> SupportedResponse:
        """Get supported payment kinds (sync — called during server initialization)."""
        data = self._do_request_sync("GET", "/supported")
        return _parse_response(data, SupportedResponse, "supported")

    async def get_settle_status(self, tx_hash: str) -> SettleStatusResponse:
        """Query settlement status by transaction hash."""
        endpoint = f"/settle/status?txHash={tx_hash}"
        data = await self._do_request_async("GET", endpoint)
        return _parse_response(data, SettleStatusResponse, "settle/status")

    async def _do_request_async(
        self, method: str, endpoint: str, body: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Perform an authenticated async HTTP request and unwrap envelope."""
        path = OKX_BASE_PATH + endpoint
        url = self._base_url + path

        body_str = json.dumps(body) if body is not None else ""
        headers = {
            "Content-Type": "application/json",
            **self._auth.create_headers(method, path, body_str),
        }

        client = self._get_async_client()
        if method == "GET":
            response = await client.get(url, headers=headers)
        else:
            response = await client.post(url, headers=headers, content=body_str)

        if response.status_code < 200 or response.status_code >= 300:
            raise OKXFacilitatorResponseError(
                f"OKX API error from {endpoint}: HTTP {response.status_code}: {response.text}"
            )

        response_data = response.json()
        return _unwrap_envelope(response_data, endpoint)

    def _do_request_sync(
        self, method: str, endpoint: str, body: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Perform an authenticated sync HTTP request (used by get_supported during init)."""
        path = OKX_BASE_PATH + endpoint
        url = self._base_url + path

        body_str = json.dumps(body) if body is not None else ""
        headers = {
            "Content-Type": "application/json",
            **self._auth.create_headers(method, path, body_str),
        }

        with self._get_sync_client() as client:
            if method == "GET":
                response = client.get(url, headers=headers)
            else:
                response = client.post(url, headers=headers, content=body_str)

            if response.status_code < 200 or response.status_code >= 300:
                raise OKXFacilitatorResponseError(
                    f"OKX API error from {endpoint}: HTTP {response.status_code}: {response.text}"
                )

            response_data = response.json()
        return _unwrap_envelope(response_data, endpoint)


# ============================================================================
# Sync OKX Facilitator Client
# ============================================================================


class OKXFacilitatorClientSync:
    """Sync OKX facilitator client satisfying the FacilitatorClientSync protocol.

    Communicates with OKX API at /api/v6/pay/x402/* using httpx.Client.
    """

    def __init__(self, config: OKXFacilitatorConfig) -> None:
        _validate_config(config)
        self._auth = OKXAuthProvider(config.auth)
        base_url = config.base_url.strip() if config.base_url else ""
        self._base_url = (base_url or OKX_DEFAULT_BASE_URL).rstrip("/")
        self._sync_settle = config.sync_settle
        self._timeout = config.timeout if config.timeout else 30.0
        self._http_client = config.http_client
        self._owns_client = config.http_client is None

    def _get_client(self) -> Any:
        if self._http_client is None:
            import httpx

            self._http_client = httpx.Client(timeout=self._timeout, follow_redirects=True)
        return self._http_client

    def close(self) -> None:
        if self._owns_client and self._http_client:
            self._http_client.close()
            self._http_client = None

    def __enter__(self) -> OKXFacilitatorClientSync:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    def verify(
        self,
        payload: PaymentPayload | PaymentPayloadV1,
        requirements: PaymentRequirements | PaymentRequirementsV1,
    ) -> VerifyResponse:
        """Verify a payment via the OKX API."""
        body = _build_body(
            _dump_model(payload),
            _dump_model(requirements),
            include_sync_settle=False,
            sync_settle=False,
        )
        data = self._do_request("POST", "/verify", body)
        return _parse_response(data, VerifyResponse, "verify")

    def settle(
        self,
        payload: PaymentPayload | PaymentPayloadV1,
        requirements: PaymentRequirements | PaymentRequirementsV1,
    ) -> SettleResponse:
        """Settle a payment via the OKX API."""
        body = _build_body(
            _dump_model(payload),
            _dump_model(requirements),
            include_sync_settle=True,
            sync_settle=self._sync_settle,
        )
        data = self._do_request("POST", "/settle", body)
        return _parse_response(data, SettleResponse, "settle")

    def get_supported(self) -> SupportedResponse:
        """Get supported payment kinds."""
        data = self._do_request("GET", "/supported")
        return _parse_response(data, SupportedResponse, "supported")

    def get_settle_status(self, tx_hash: str) -> SettleStatusResponse:
        """Query settlement status by transaction hash."""
        endpoint = f"/settle/status?txHash={tx_hash}"
        data = self._do_request("GET", endpoint)
        return _parse_response(data, SettleStatusResponse, "settle/status")

    def _do_request(
        self, method: str, endpoint: str, body: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Perform an authenticated sync HTTP request and unwrap envelope."""
        path = OKX_BASE_PATH + endpoint
        url = self._base_url + path

        body_str = json.dumps(body) if body is not None else ""
        headers = {
            "Content-Type": "application/json",
            **self._auth.create_headers(method, path, body_str),
        }

        client = self._get_client()
        if method == "GET":
            response = client.get(url, headers=headers)
        else:
            response = client.post(url, headers=headers, content=body_str)

        if response.status_code < 200 or response.status_code >= 300:
            raise OKXFacilitatorResponseError(
                f"OKX API error from {endpoint}: HTTP {response.status_code}: {response.text}"
            )

        response_data = response.json()
        return _unwrap_envelope(response_data, endpoint)


# ============================================================================
# Response parsing helper
# ============================================================================


def _parse_response(data: dict[str, Any], model_cls: type, operation: str) -> Any:
    """Parse unwrapped response data into a validated pydantic model."""
    try:
        return model_cls.model_validate(data)
    except (ValidationError, ValueError, TypeError) as exc:
        excerpt = json.dumps(data)[:200] if data else "<empty>"
        raise OKXFacilitatorResponseError(
            f"OKX {operation} returned invalid data: {excerpt}"
        ) from exc
