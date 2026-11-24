"""Tests for paymentrouter.merger — concurrent challenge header merging."""

from __future__ import annotations

import pytest
from starlette.requests import Request

from paymentrouter.merger import merge_challenges

from .conftest import MockAdapter


def _make_request() -> Request:
    scope = {"type": "http", "method": "GET", "path": "/", "headers": [], "query_string": b""}
    return Request(scope)


class TestMergeChallenges:
    @pytest.mark.asyncio
    async def test_merges_headers_from_multiple_adapters(self) -> None:
        """Two adapters return different headers, both present in result."""
        a1 = MockAdapter(name="a1", challenge_result={"X-Proto-A": "val-a"})
        a2 = MockAdapter(name="a2", challenge_result={"X-Proto-B": "val-b"})
        cfg = {"a1": {}, "a2": {}}

        merged = await merge_challenges([a1, a2], _make_request(), cfg)

        assert "X-Proto-A" in merged
        assert "X-Proto-B" in merged
        assert merged["X-Proto-A"] == ["val-a"]
        assert merged["X-Proto-B"] == ["val-b"]

    @pytest.mark.asyncio
    async def test_multi_value_same_header_key(self) -> None:
        """Two adapters return same header key, values accumulated in list."""
        a1 = MockAdapter(name="a1", challenge_result={"WWW-Authenticate": "Bearer realm=a1"})
        a2 = MockAdapter(name="a2", challenge_result={"WWW-Authenticate": "X402 realm=a2"})
        cfg = {"a1": {}, "a2": {}}

        merged = await merge_challenges([a1, a2], _make_request(), cfg)

        assert "WWW-Authenticate" in merged
        assert len(merged["WWW-Authenticate"]) == 2
        assert "Bearer realm=a1" in merged["WWW-Authenticate"]
        assert "X402 realm=a2" in merged["WWW-Authenticate"]

    @pytest.mark.asyncio
    async def test_filters_to_route_cfg_adapters_only(self) -> None:
        """Adapter not in route_cfg is skipped."""
        a1 = MockAdapter(name="present", challenge_result={"X-Present": "yes"})
        a2 = MockAdapter(name="absent", challenge_result={"X-Absent": "no"})
        cfg = {"present": {}}

        merged = await merge_challenges([a1, a2], _make_request(), cfg)

        assert "X-Present" in merged
        assert "X-Absent" not in merged

    @pytest.mark.asyncio
    async def test_single_adapter_error_doesnt_block_others(self) -> None:
        """One adapter raises, other succeeds, on_error called."""
        a_bad = MockAdapter(name="bad", challenge_result=RuntimeError("fail"))
        a_good = MockAdapter(name="good", challenge_result={"X-Ok": "ok"})
        cfg = {"bad": {}, "good": {}}

        errors: list[tuple[Exception, str]] = []

        def _on_error(exc: Exception, name: str) -> None:
            errors.append((exc, name))

        merged = await merge_challenges([a_bad, a_good], _make_request(), cfg, on_error=_on_error)

        assert "X-Ok" in merged
        assert merged["X-Ok"] == ["ok"]
        assert len(errors) == 1
        assert errors[0][1] == "bad"

    @pytest.mark.asyncio
    async def test_all_adapters_error_returns_empty(self) -> None:
        """Both fail, returns empty dict."""
        a1 = MockAdapter(name="a1", challenge_result=RuntimeError("fail1"))
        a2 = MockAdapter(name="a2", challenge_result=ValueError("fail2"))
        cfg = {"a1": {}, "a2": {}}

        merged = await merge_challenges([a1, a2], _make_request(), cfg)

        assert merged == {}

    @pytest.mark.asyncio
    async def test_on_error_callback_receives_exception_and_name(self) -> None:
        """Verify callback args: (exception, adapter_name)."""
        error = ValueError("specific error")
        a = MockAdapter(name="erroring", challenge_result=error)
        cfg = {"erroring": {}}

        captured: list[tuple[Exception, str]] = []

        def _on_error(exc: Exception, name: str) -> None:
            captured.append((exc, name))

        await merge_challenges([a], _make_request(), cfg, on_error=_on_error)

        assert len(captured) == 1
        exc, name = captured[0]
        assert name == "erroring"
        assert isinstance(exc, ValueError)
        assert str(exc) == "specific error"

    @pytest.mark.asyncio
    async def test_none_challenge_result_skipped(self) -> None:
        """Adapter returning None headers is silently skipped."""
        a_nil = MockAdapter(name="nilheader", challenge_result=None)
        a_real = MockAdapter(name="realheader", challenge_result={"X-Real": "v"})
        cfg = {"nilheader": {}, "realheader": {}}

        merged = await merge_challenges([a_nil, a_real], _make_request(), cfg)

        assert "X-Real" in merged
        assert merged["X-Real"] == ["v"]

    @pytest.mark.asyncio
    async def test_empty_adapters_returns_empty(self) -> None:
        """No adapters returns empty dict."""
        merged = await merge_challenges([], _make_request(), {})

        assert merged == {}
