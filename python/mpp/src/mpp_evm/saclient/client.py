"""OKX Settlement Agent API client with HMAC-SHA256 authentication.

"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
from datetime import datetime, timezone
from typing import Any, Protocol, runtime_checkable
from urllib.parse import urlencode

import httpx

from mpp_evm.errors import InternalSAError, map_sa_error
from mpp_evm.saclient.types import (
    ChargeReceipt,
    ChargeSettleRequest,
    ChargeVerifyHashRequest,
    SessionCloseRequest,
    SessionOpenRequest,
    SessionReceipt,
    SessionSettleRequest,
    SessionStatus,
    SessionTopUpRequest,
)


# ---------------------------------------------------------------------------
# SAClient protocol (interface)
# ---------------------------------------------------------------------------


@runtime_checkable
class SAClient(Protocol):
    """Protocol for Settlement Agent API clients."""

    async def settle(self, req: ChargeSettleRequest) -> ChargeReceipt: ...

    async def verify_hash(self, req: ChargeVerifyHashRequest) -> ChargeReceipt: ...

    async def session_open(self, req: SessionOpenRequest) -> SessionReceipt: ...

    async def session_top_up(self, req: SessionTopUpRequest) -> SessionReceipt: ...

    async def session_settle(self, req: SessionSettleRequest) -> SessionReceipt: ...

    async def session_close(self, req: SessionCloseRequest) -> SessionReceipt: ...

    async def session_status(self, channel_id: str) -> SessionStatus: ...


# ---------------------------------------------------------------------------
# OKXSAClient implementation
# ---------------------------------------------------------------------------

_PATH_CHARGE_SETTLE = "/api/v6/pay/mpp/charge/settle"
_PATH_CHARGE_VERIFY_HASH = "/api/v6/pay/mpp/charge/verifyHash"
_PATH_SESSION_OPEN = "/api/v6/pay/mpp/session/open"
_PATH_SESSION_TOP_UP = "/api/v6/pay/mpp/session/topUp"
_PATH_SESSION_SETTLE = "/api/v6/pay/mpp/session/settle"
_PATH_SESSION_CLOSE = "/api/v6/pay/mpp/session/close"
_PATH_SESSION_STATUS = "/api/v6/pay/mpp/session/status"


class OKXSAClient:
    """OKX Settlement Agent API client with HMAC-SHA256 authentication.

    Authenticates via OKX API key + HMAC-SHA256 signature + passphrase.

    """

    def __init__(
        self,
        *,
        base_url: str,
        api_key: str,
        secret_key: str,
        passphrase: str,
        http_client: httpx.AsyncClient | None = None,
        timeout: float = 30.0,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._secret_key = secret_key
        self._passphrase = passphrase
        self._timeout = timeout
        self._client = http_client
        # Only close the client on cleanup if we created it ourselves; an
        # injected client is owned by the caller.
        self._owns_client = http_client is None

    async def _get_client(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = httpx.AsyncClient(timeout=self._timeout)
        return self._client

    async def aclose(self) -> None:
        """Close the underlying HTTP client if this instance created it."""
        if self._client is not None and self._owns_client:
            await self._client.aclose()
            self._client = None

    async def __aenter__(self) -> "OKXSAClient":
        return self

    async def __aexit__(self, *exc: object) -> None:
        await self.aclose()


    def _sign(self, timestamp: str, method: str, request_path: str, body: str) -> str:
        """Compute OKX API HMAC-SHA256 signature.

        Format: base64(HMAC-SHA256(secretKey, timestamp + method + requestPath + body))
        """
        prehash = timestamp + method + request_path + body
        mac = hmac.new(self._secret_key.encode(), prehash.encode(), hashlib.sha256)
        return base64.b64encode(mac.digest()).decode()

    def _auth_headers(self, method: str, request_path: str, body: str) -> dict[str, str]:
        """Build OKX API authentication headers."""
        timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        signature = self._sign(timestamp, method, request_path, body)
        return {
            "OK-ACCESS-KEY": self._api_key,
            "OK-ACCESS-SIGN": signature,
            "OK-ACCESS-TIMESTAMP": timestamp,
            "OK-ACCESS-PASSPHRASE": self._passphrase,
            "Content-Type": "application/json",
        }


    async def _do_post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        """Marshal body to JSON, POST to path, return parsed data field."""
        body = json.dumps(payload, separators=(",", ":"))
        headers = self._auth_headers("POST", path, body)
        client = await self._get_client()
        url = self._base_url + path

        resp = await client.post(url, content=body, headers=headers)
        return self._handle_response(resp)

    async def _do_get(self, path: str, params: dict[str, str]) -> dict[str, Any]:
        """Issue GET to path with query params, return parsed data field."""
        request_path = path
        if params:
            query = urlencode(sorted(params.items()))
            request_path = path + "?" + query

        headers = self._auth_headers("GET", request_path, "")
        client = await self._get_client()
        url = self._base_url + request_path

        resp = await client.get(url, headers=headers)
        return self._handle_response(resp)

    def _handle_response(self, resp: httpx.Response) -> dict[str, Any]:
        """Parse SA response envelope, raise on non-zero code."""
        if resp.status_code != 200:
            raise InternalSAError(
                detail=f"SA API returned HTTP {resp.status_code}",
                reason=resp.text[:500],
            )

        try:
            data = resp.json()
        except (json.JSONDecodeError, ValueError) as exc:
            raise InternalSAError(
                detail="Failed to decode SA API response",
                reason=str(exc),
            )

        code = data.get("code", 0)
        if isinstance(code, str):
            code = int(code) if code.isdigit() else 0
        if code != 0:
            msg = data.get("msg", "")
            raise map_sa_error(code, msg)

        return data.get("data") or {}


    async def settle(self, req: ChargeSettleRequest) -> ChargeReceipt:
        """Submit a charge for on-chain settlement.

        POST /api/v6/pay/mpp/charge/settle
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_CHARGE_SETTLE, payload)
        return ChargeReceipt.model_validate(data)

    async def verify_hash(self, req: ChargeVerifyHashRequest) -> ChargeReceipt:
        """Verify a client-broadcast transaction hash.

        POST /api/v6/pay/mpp/charge/verifyHash
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_CHARGE_VERIFY_HASH, payload)
        return ChargeReceipt.model_validate(data)


    async def session_open(self, req: SessionOpenRequest) -> SessionReceipt:
        """Open a new payment channel session.

        POST /api/v6/pay/mpp/session/open
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_SESSION_OPEN, payload)
        return SessionReceipt.model_validate(data)

    async def session_top_up(self, req: SessionTopUpRequest) -> SessionReceipt:
        """Add deposit to an existing payment channel.

        POST /api/v6/pay/mpp/session/topUp
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_SESSION_TOP_UP, payload)
        return SessionReceipt.model_validate(data)

    async def session_settle(self, req: SessionSettleRequest) -> SessionReceipt:
        """Trigger intermediate on-chain settlement.

        POST /api/v6/pay/mpp/session/settle
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_SESSION_SETTLE, payload)
        return SessionReceipt.model_validate(data)

    async def session_close(self, req: SessionCloseRequest) -> SessionReceipt:
        """Finalize and close a payment channel.

        POST /api/v6/pay/mpp/session/close
        """
        payload = req.model_dump(by_alias=True, exclude_none=True)
        data = await self._do_post(_PATH_SESSION_CLOSE, payload)
        return SessionReceipt.model_validate(data)

    async def session_status(self, channel_id: str) -> SessionStatus:
        """Query the current state of a payment channel.

        GET /api/v6/pay/mpp/session/status?channelId=...
        """
        data = await self._do_get(_PATH_SESSION_STATUS, {"channelId": channel_id})
        return SessionStatus.model_validate(data)
