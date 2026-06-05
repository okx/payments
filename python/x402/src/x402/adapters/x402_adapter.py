"""PaymentRouter ProtocolAdapter for x402.

Mirrors go/x402/adapters/x402_adapter.go.
Wraps an x402HTTPResourceServer and delegates detect/challenge/handle.
"""

from __future__ import annotations

from typing import Any

from x402.http.x402_http_server import x402HTTPResourceServer
from x402.http.constants import PAYMENT_SIGNATURE_HEADER
from x402.http.types import (
    HTTPAdapter,
    HTTPRequestContext,
    RouteConfig,
    RESULT_NO_PAYMENT_REQUIRED,
    RESULT_PAYMENT_VERIFIED,
)


class _RequestAdapterBridge:
    """Bridges paymentrouter's RequestAdapter to x402's HTTPAdapter protocol."""

    def __init__(self, request: Any) -> None:
        self._request = request

    def get_header(self, name: str) -> str:
        return self._request.get_header(name)

    def get_method(self) -> str:
        return self._request.method

    def get_path(self) -> str:
        return self._request.path

    def get_url(self) -> str:
        return self._request.url

    def get_accept_header(self) -> str:
        return self._request.get_header("accept")

    def get_user_agent(self) -> str:
        return self._request.get_header("user-agent")


class X402Adapter:
    """PaymentRouter ProtocolAdapter for x402 protocol.

    Wraps an x402HTTPResourceServer. Detects requests with
    PAYMENT-SIGNATURE or X-Payment header.
    """

    def __init__(self, server: x402HTTPResourceServer, priority: int = 20) -> None:
        self._server = server
        self._priority = priority
        self._registered_routes: set[str] = set()

    @property
    def name(self) -> str:
        return "x402"

    @property
    def priority(self) -> int:
        return self._priority

    def detect(self, request: Any) -> bool:
        return bool(
            request.get_header(PAYMENT_SIGNATURE_HEADER)
            or request.get_header(PAYMENT_SIGNATURE_HEADER.lower())
            or request.get_header("x-payment")
        )

    def _maybe_initialize(self) -> None:
        if not getattr(self, "_initialized", False):
            self._server._server.initialize()
            self._initialized = True

    def _ensure_route(self, method: str | Any, path: str | Any, cfg: Any) -> None:
        endpoint = f"{method} {path}"
        if endpoint in self._registered_routes:
            return
        if isinstance(cfg, RouteConfig):
            self._server.add_routes({endpoint: cfg})
            self._registered_routes.add(endpoint)

    async def get_challenge(self, request: Any, cfg: Any) -> dict[str, str]:
        """Generate 402 challenge.

        Tries the x402 server first. Falls back to building the header
        directly from RouteConfig if the server can't resolve options.
        """
        self._ensure_route(request.method, request.path, cfg)
        self._maybe_initialize()

        # Try server-generated challenge first
        bridge = _RequestAdapterBridge(request)
        context = HTTPRequestContext(
            adapter=bridge,
            path=request.path,
            method=request.method,
        )

        result = await self._server.process_http_request(context)
        if result.response is not None and result.response.headers:
            return dict(result.response.headers)

        # Fallback: build challenge directly from RouteConfig
        if not isinstance(cfg, RouteConfig):
            return {}
        return self._build_challenge_from_config(cfg, request)

    def _build_challenge_from_config(self, cfg: RouteConfig, request: Any) -> dict[str, str]:
        """Build PAYMENT-REQUIRED header from RouteConfig as fallback."""
        from x402.http.utils import encode_payment_required_header
        from x402.schemas import PaymentRequired, PaymentRequirements

        requirements_list = []
        for opt in cfg.accepts if isinstance(cfg.accepts, list) else [cfg.accepts]:
            price_str = str(opt.price)
            if price_str.startswith("$"):
                price_str = price_str[1:]
            req = PaymentRequirements(
                scheme=opt.scheme,
                network=opt.network,
                asset=opt.extra.get("asset", "") if opt.extra else "",
                amount=price_str,
                pay_to=opt.pay_to if isinstance(opt.pay_to, str) else "",
                max_timeout_seconds=opt.max_timeout_seconds or 300,
                extra=opt.extra or {},
            )
            requirements_list.append(req)

        if not requirements_list:
            return {}

        payment_required = PaymentRequired(accepts=requirements_list)
        header_value = encode_payment_required_header(payment_required)
        return {"PAYMENT-REQUIRED": header_value}

    async def handle(self, request: Any, cfg: Any) -> Any:
        self._ensure_route(request.method, request.path, cfg)
        self._maybe_initialize()

        bridge = _RequestAdapterBridge(request)
        context = HTTPRequestContext(
            adapter=bridge,
            path=request.path,
            method=request.method,
        )

        result = await self._server.process_http_request(context)

        if result.type == RESULT_PAYMENT_VERIFIED:
            # Process settlement
            if result.payment_payload is not None and result.payment_requirements is not None:
                settle_result = await self._server.process_settlement(
                    result.payment_payload, result.payment_requirements, context
                )
                if settle_result and not settle_result.success:
                    raise ValueError(f"x402: settlement failed: {settle_result.error_reason}")
                return {"settled": True, "headers": settle_result.headers if settle_result else {}}
            return {"verified": True}

        if result.type == RESULT_NO_PAYMENT_REQUIRED:
            return {"no_payment_required": True}

        # Payment error — raise so paymentrouter returns 402
        raise ValueError(f"x402: payment error")
