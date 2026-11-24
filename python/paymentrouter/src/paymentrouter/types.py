"""Foundational types for the PaymentRouter package.

Mirrors Go paymentrouter/types.go — RouteConfig and RouteEntry.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

# Maps adapter name to adapter-specific config. Each adapter type-checks
# its own config value in get_challenge/handle.
RouteConfig = dict[str, Any]


@dataclass
class RouteEntry:
    """A single route with a URL pattern and per-adapter configuration.

    Attributes:
        pattern: Route matching pattern. Supports formats:
            - "METHOD /path" (e.g. "GET /api/resource")
            - "/path" (matches any method)
            - "*" (matches all routes)
        config: Mapping of adapter name to adapter-specific configuration.
    """

    pattern: str
    config: RouteConfig
