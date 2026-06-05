"""Tests for paymentrouter.detector — priority-ordered protocol detection."""

from __future__ import annotations

from starlette.requests import Request

from paymentrouter.detector import detect

from .conftest import MockAdapter


def _make_request() -> Request:
    scope = {"type": "http", "method": "GET", "path": "/", "headers": [], "query_string": b""}
    return Request(scope)


class TestDetect:
    def test_returns_highest_priority_match(self) -> None:
        """Two adapters both detect; lowest priority value wins."""
        a1 = MockAdapter(name="high", priority=10, detect_result=True)
        a2 = MockAdapter(name="low", priority=1, detect_result=True)

        result = detect([a1, a2], _make_request())

        assert result is not None
        assert result.name == "low"

    def test_returns_none_when_no_match(self) -> None:
        """No adapter detects — returns None."""
        a1 = MockAdapter(name="x", priority=1, detect_result=False)
        a2 = MockAdapter(name="y", priority=2, detect_result=False)

        result = detect([a1, a2], _make_request())

        assert result is None

    def test_exception_in_detect_treated_as_no_match(self) -> None:
        """Adapter raising in detect() is skipped; next one matches."""

        def _raise(request: Request) -> bool:
            raise RuntimeError("boom")

        a1 = MockAdapter(name="panicky", priority=1, detect_result=_raise)
        a2 = MockAdapter(name="safe", priority=2, detect_result=True)

        result = detect([a1, a2], _make_request())

        assert result is not None
        assert result.name == "safe"

    def test_single_adapter_match(self) -> None:
        """Single adapter that detects successfully."""
        a = MockAdapter(name="only", priority=5, detect_result=True)

        result = detect([a], _make_request())

        assert result is not None
        assert result.name == "only"

    def test_priority_ordering(self) -> None:
        """Verify sorting is ascending — lower number = higher priority."""
        a1 = MockAdapter(name="p10", priority=10, detect_result=True)
        a2 = MockAdapter(name="p5", priority=5, detect_result=True)
        a3 = MockAdapter(name="p1", priority=1, detect_result=True)

        result = detect([a1, a2, a3], _make_request())

        assert result is not None
        assert result.name == "p1"

    def test_empty_adapters_returns_none(self) -> None:
        """Empty adapter list returns None."""
        result = detect([], _make_request())

        assert result is None
