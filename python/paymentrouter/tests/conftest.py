"""Mock adapter helper for PaymentRouter tests."""

from __future__ import annotations

from typing import Any, Callable

from starlette.requests import Request

from paymentrouter.adapter import ProtocolAdapter


class MockAdapter:
    """Configurable mock implementing ProtocolAdapter for unit tests.

    Args:
        name: Adapter identifier.
        priority: Detection priority (lower = higher priority).
        detect_result: Bool or callable(Request) -> bool for detect().
        challenge_result: Dict to return from get_challenge(), or an exception to raise.
        handle_result: Value to return from handle(), or an exception to raise.
    """

    def __init__(
        self,
        *,
        name: str = "mock",
        priority: int = 0,
        detect_result: bool | Callable[[Request], bool] = False,
        challenge_result: dict[str, str] | Exception | None = None,
        handle_result: Any | Exception = None,
    ) -> None:
        self._name = name
        self._priority = priority
        self._detect_result = detect_result
        self._challenge_result = challenge_result
        self._handle_result = handle_result

    @property
    def name(self) -> str:
        return self._name

    @property
    def priority(self) -> int:
        return self._priority

    def detect(self, request: Request) -> bool:
        if callable(self._detect_result) and not isinstance(self._detect_result, bool):
            return self._detect_result(request)
        return self._detect_result

    async def get_challenge(self, request: Request, cfg: Any) -> dict[str, str] | None:
        if isinstance(self._challenge_result, Exception):
            raise self._challenge_result
        return self._challenge_result

    async def handle(self, request: Request, cfg: Any) -> Any:
        if isinstance(self._handle_result, Exception):
            raise self._handle_result
        return self._handle_result


assert isinstance(MockAdapter(), ProtocolAdapter)
