"""Priority-ordered protocol detection.

Mirrors Go paymentrouter/detector.go — Detect() function.
"""

from __future__ import annotations

import logging
from typing import Sequence

from starlette.requests import Request

from paymentrouter.adapter import ProtocolAdapter

logger = logging.getLogger(__name__)


def detect(
    protocols: Sequence[ProtocolAdapter],
    request: Request,
) -> ProtocolAdapter | None:
    """Return the first matching adapter sorted by ascending priority.

    Each adapter's detect() is called in priority order (lowest first).
    Exceptions raised by an adapter's detect() are caught and treated as
    a non-match, mirroring Go's panic-recovery semantics.

    Args:
        protocols: Available protocol adapters.
        request: The incoming HTTP request.

    Returns:
        The first adapter whose detect() returns True, or None.
    """
    sorted_protocols = sorted(protocols, key=lambda a: a.priority)

    for adapter in sorted_protocols:
        try:
            if adapter.detect(request):
                return adapter
        except Exception:
            logger.debug(
                "Adapter %r raised during detect, treating as no-match",
                adapter.name,
            )
    return None
