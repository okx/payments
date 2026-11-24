"""Concurrent challenge header merging.

Mirrors Go paymentrouter/merger.go — MergeChallenges() function.
"""

from __future__ import annotations

import asyncio
from collections.abc import Callable, Sequence
from typing import Any

from starlette.requests import Request

from paymentrouter.adapter import ProtocolAdapter
from paymentrouter.types import RouteConfig


async def merge_challenges(
    protocols: Sequence[ProtocolAdapter],
    request: Request,
    route_cfg: RouteConfig,
    on_error: Callable[[Exception, str], Any] | None = None,
) -> dict[str, list[str]]:
    """Gather challenge headers from all configured adapters concurrently.

    Filters adapters to those whose name appears as a key in route_cfg,
    then calls get_challenge() on each concurrently via asyncio.gather.
    Results are merged such that the same header key accumulates into a
    list of values (mirrors Go's http.Header.Add semantics).

    Args:
        protocols: Available protocol adapters.
        request: The incoming HTTP request.
        route_cfg: Mapping of adapter name to adapter-specific config.
        on_error: Optional callback invoked with (exception, adapter_name)
            when an adapter's get_challenge() fails.

    Returns:
        Merged headers as a dict mapping header name to list of values.
    """
    filtered = [a for a in protocols if a.name in route_cfg]

    if not filtered:
        return {}

    async def _get(
        adapter: ProtocolAdapter,
    ) -> tuple[ProtocolAdapter, dict[str, str] | None, Exception | None]:
        try:
            headers = await adapter.get_challenge(request, route_cfg[adapter.name])
            return adapter, headers, None
        except Exception as exc:
            return adapter, None, exc

    results = await asyncio.gather(*[_get(a) for a in filtered])

    merged: dict[str, list[str]] = {}
    for adapter, headers, err in results:
        if err is not None:
            if on_error is not None:
                on_error(err, adapter.name)
            continue
        if headers is None:
            continue
        for key, value in headers.items():
            merged.setdefault(key, []).append(value)

    return merged
