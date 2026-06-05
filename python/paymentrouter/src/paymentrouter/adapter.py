"""Protocol adapter interface for the PaymentRouter package.

Mirrors Go paymentrouter/adapter.go — ProtocolAdapter interface.

Framework-agnostic: adapters receive a RequestAdapter, not a
framework-specific request object. Each framework middleware
wraps its request type into a RequestAdapter.
"""

from __future__ import annotations

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class RequestAdapter(Protocol):
    """Framework-agnostic HTTP request interface.

    Framework middlewares wrap their request types into this.
    Similar to x402's HTTPAdapter pattern.
    """

    @property
    def method(self) -> str: ...

    @property
    def path(self) -> str: ...

    @property
    def url(self) -> str: ...

    def get_header(self, name: str) -> str: ...


@runtime_checkable
class ProtocolAdapter(Protocol):
    """Interface that payment protocol adapters must implement.

    Adapters are registered with the router and invoked in priority order
    during request processing. Each adapter handles a single payment protocol
    (e.g. x402, MPP).
    """

    @property
    def name(self) -> str:
        """Unique identifier for this adapter (e.g. "x402", "mpp")."""
        ...

    @property
    def priority(self) -> int:
        """Detection priority — lower values are checked first."""
        ...

    def detect(self, request: RequestAdapter) -> bool:
        """Determine whether this adapter's protocol is present in the request."""
        ...

    async def get_challenge(self, request: RequestAdapter, cfg: Any) -> dict[str, str]:
        """Generate challenge headers for a 402 Payment Required response."""
        ...

    async def handle(self, request: RequestAdapter, cfg: Any) -> Any:
        """Process a payment request. Returns result on success, raises on failure."""
        ...
