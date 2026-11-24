"""FastAPI/Starlette integration for PaymentRouter.

Mirrors Go paymentrouter/gin/middleware.go — PaymentGate with For().

Provides two integration styles:
- PaymentGate.for_route(cfg) returns a Depends-compatible callable for per-route use
- PaymentMiddleware is ASGI middleware for declarative app-wide route protection

Per-route usage::

    from paymentrouter.fastapi import PaymentGate

    paid = PaymentGate([mpp_adapter, x402_adapter])

    @app.get("/resource", dependencies=[Depends(paid.for_route(cfg))])
    async def handler():
        ...

App-wide usage::

    from paymentrouter.fastapi import PaymentMiddleware

    app.add_middleware(PaymentMiddleware, gate=paid, routes={
        "GET /api/weather": {"mpp": mpp_cfg, "x402": x402_cfg},
    })
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from fastapi import Request
from fastapi.responses import JSONResponse, Response
from starlette.types import ASGIApp, Receive, Scope, Send

from paymentrouter.adapter import ProtocolAdapter, RequestAdapter
from paymentrouter.detector import detect
from paymentrouter.merger import merge_challenges
from paymentrouter.router import Router
from paymentrouter.types import RouteConfig


class _FastAPIRequestAdapter:
    """Wraps fastapi.Request into framework-agnostic RequestAdapter."""

    def __init__(self, request: Request) -> None:
        self._request = request

    @property
    def method(self) -> str:
        return self._request.method

    @property
    def path(self) -> str:
        return self._request.url.path

    @property
    def url(self) -> str:
        return str(self._request.url)

    def get_header(self, name: str) -> str:
        return self._request.headers.get(name, "")

OnErrorCallback = Callable[[Exception, str, str], None]


class _ManagementResponse(Exception):
    """Raised to short-circuit management actions (open/topUp/close/settle)."""

    def __init__(self, content: Any) -> None:
        self.content = content


class _PaymentErrorResponse(Exception):
    """Raised to return payment errors with proper status code and body."""

    def __init__(self, status_code: int, content: Any) -> None:
        self.status_code = status_code
        self.content = content


class _ChallengeResponse(Exception):
    """Raised to return a 402 with merged challenge headers.

    Carries the merged ``dict[str, list[str]]`` so the handler can emit each
    value as its own header line (multiple ``WWW-Authenticate`` challenges must
    not be comma-folded — see ``_apply_merged_headers``).
    """

    def __init__(self, merged: dict[str, list[str]]) -> None:
        self.merged = merged


def _apply_merged_headers(response: Response, merged: dict[str, list[str]]) -> None:
    """Append each merged header value as a separate header line.

    Multiple same-named headers (notably ``WWW-Authenticate`` when several
    adapters challenge) MUST be emitted as distinct lines, not joined with
    ``", "`` — RFC 7235 challenge values contain their own commas, so a folded
    line is ambiguous. Mirrors Go's ``Header().Add`` and Rust's append.
    """
    for key, values in merged.items():
        for value in values:
            response.headers.append(key, value)


def register_management_handler(app: Any) -> None:
    """Register exception handlers for payment responses on a FastAPI app."""
    from fastapi.responses import JSONResponse as _JSONResponse

    @app.exception_handler(_ManagementResponse)
    async def _handle_management(request: Request, exc: _ManagementResponse) -> Response:
        return _JSONResponse(status_code=200, content=exc.content)

    @app.exception_handler(_PaymentErrorResponse)
    async def _handle_payment_error(request: Request, exc: _PaymentErrorResponse) -> Response:
        return _JSONResponse(status_code=exc.status_code, content=exc.content)

    @app.exception_handler(_ChallengeResponse)
    async def _handle_challenge(request: Request, exc: _ChallengeResponse) -> Response:
        response = _JSONResponse(status_code=402, content={"error": "Payment required"})
        _apply_merged_headers(response, exc.merged)
        return response


class PaymentGate:
    """Holds protocol adapters and produces per-route FastAPI dependencies.

    Args:
        protocols: List of protocol adapters to use for detection and challenges.
        on_error: Optional callback invoked as on_error(exception, phase, adapter_name)
            when handle() or get_challenge() fails. Phase is "handle" or "challenge".
    """

    def __init__(
        self,
        protocols: list[ProtocolAdapter],
        on_error: OnErrorCallback | None = None,
    ) -> None:
        self.protocols = protocols
        self.on_error = on_error

    def for_route(self, cfg: RouteConfig) -> Callable[..., Any]:
        """Return an async callable compatible with FastAPI Depends().

        The returned dependency runs the detect/handle/challenge flow
        for the given route configuration.

        Args:
            cfg: Mapping of adapter name to adapter-specific config.

        Returns:
            Async callable that accepts a Request and either passes
            (payment valid) or raises HTTPException(402).
        """
        gate = self

        async def _dependency(request: Request) -> Any:
            return await _run_gate(gate.protocols, cfg, request, gate.on_error)

        return _dependency


class PaymentMiddleware:
    """ASGI middleware that applies payment gating to matched routes.

    Args:
        app: The ASGI application to wrap.
        gate: PaymentGate instance providing adapters and error callback.
        routes: Mapping of route patterns to RouteConfig. Patterns follow
            the same format as Router: "METHOD /path", "/path", or "*".
    """

    def __init__(
        self,
        app: ASGIApp,
        gate: PaymentGate,
        routes: dict[str, RouteConfig],
    ) -> None:
        self.app = app
        self.gate = gate
        self.router = Router(routes)

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return

        method = scope.get("method", "GET")
        path = scope.get("path", "/")

        result = self.router.match(method, path)
        if result is None:
            await self.app(scope, receive, send)
            return

        route_cfg, _ = result
        request = Request(scope, receive, send)
        wrapped = _FastAPIRequestAdapter(request)

        adapter = detect(self.gate.protocols, wrapped)
        if adapter is not None:
            adapter_cfg = route_cfg.get(adapter.name, {})
            try:
                handle_result = await adapter.handle(wrapped, adapter_cfg)
            except Exception as exc:
                if self.gate.on_error is not None:
                    self.gate.on_error(exc, "handle", adapter.name)
                response = JSONResponse(
                    {"error": "Payment processing failed"},
                    status_code=400,
                )
                await response(scope, receive, send)
                return
            # Session-management credential (open/topUp/close/settle): return the
            # management response and do NOT serve the protected resource. Mirrors
            # the Depends path (_run_gate); without this, a management credential
            # would pass the gate and reach the resource without a voucher payment.
            if isinstance(handle_result, dict) and handle_result.get("_management"):
                response = JSONResponse(status_code=200, content=handle_result["response"])
                await response(scope, receive, send)
                return
            request.state.payment_result = handle_result
            await self.app(scope, receive, send)
            return

        def _on_challenge_error(exc: Exception, adapter_name: str) -> None:
            if self.gate.on_error is not None:
                self.gate.on_error(exc, "challenge", adapter_name)

        merged = await merge_challenges(
            self.gate.protocols, wrapped, route_cfg, on_error=_on_challenge_error
        )

        response = _build_402_response(merged)
        await response(scope, receive, send)


async def _run_gate(
    protocols: list[ProtocolAdapter],
    cfg: RouteConfig,
    request: Request,
    on_error: OnErrorCallback | None,
) -> Any:
    """Core gate logic shared by PaymentGate.for_route dependency."""
    from fastapi import HTTPException

    wrapped = _FastAPIRequestAdapter(request)

    adapter = detect(protocols, wrapped)
    if adapter is not None:
        adapter_cfg = cfg.get(adapter.name, {})
        try:
            result = await adapter.handle(wrapped, adapter_cfg)
            request.state.payment_result = result
            # Management actions (session open/topUp/close) short-circuit:
            # raise to prevent handler from running.
            if isinstance(result, dict) and result.get("_management"):
                raise _ManagementResponse(content=result["response"])
            return result
        except (_ManagementResponse, _PaymentErrorResponse):
            raise
        except Exception as exc:
            if on_error is not None:
                on_error(exc, "handle", adapter.name)
            # Forward payment errors with proper status code and body
            status = getattr(exc, "status_code", None)
            if status is not None:
                if hasattr(exc, "to_problem_details"):
                    raise _PaymentErrorResponse(
                        status_code=status, content=exc.to_problem_details()
                    ) from exc
                raise HTTPException(status_code=status, detail=str(exc)) from exc
            raise HTTPException(
                status_code=400,
                detail="Payment processing failed",
            ) from exc

    def _on_challenge_error(exc: Exception, adapter_name: str) -> None:
        if on_error is not None:
            on_error(exc, "challenge", adapter_name)

    merged = await merge_challenges(
        protocols, wrapped, cfg, on_error=_on_challenge_error
    )

    # When no header name carries more than one value, HTTPException's dict
    # headers are sufficient and lossless — keep the native path (works without
    # register_management_handler). Only when an adapter contributes duplicate
    # header names (e.g. two WWW-Authenticate challenges) do we need separate
    # header lines, which HTTPException's dict[str, str] cannot represent — route
    # through _ChallengeResponse (handled by register_management_handler) so each
    # value is emitted as its own line rather than comma-folded.
    if any(len(values) > 1 for values in merged.values()):
        raise _ChallengeResponse(merged)

    headers = {key: values[0] for key, values in merged.items()}
    raise HTTPException(
        status_code=402,
        detail="Payment required",
        headers=headers,
    )


def _build_402_response(merged: dict[str, list[str]]) -> Response:
    """Build a 402 response with merged challenge headers (multi-line)."""
    response = JSONResponse(
        {"error": "Payment required"},
        status_code=402,
    )
    _apply_merged_headers(response, merged)
    return response
