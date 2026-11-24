"""Compiled regex route matching.

Mirrors Go paymentrouter/router.go — parses route keys, compiles path
patterns to regexes, and matches incoming (method, path) pairs.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from urllib.parse import unquote

from .types import RouteConfig


@dataclass
class CompiledRoute:
    regex: re.Pattern[str]
    verb: str
    pattern: str
    config: RouteConfig


class Router:
    def __init__(self, routes: dict[str, RouteConfig]) -> None:
        self._routes = self._compile(routes)

    def match(self, method: str, path: str) -> tuple[RouteConfig, str] | None:
        path = _normalize_path(path)
        method = method.upper()
        for route in self._routes:
            if route.verb != "*" and route.verb != method:
                continue
            if route.regex.match(path):
                return (route.config, route.pattern)
        return None

    def _compile(self, routes: dict[str, RouteConfig]) -> list[CompiledRoute]:
        compiled: list[CompiledRoute] = []
        for key, config in routes.items():
            verb, path = _parse_route_key(key)
            compiled.append(CompiledRoute(
                regex=_compile_path(path),
                verb=verb,
                pattern=key,
                config=config,
            ))
        return compiled


def _parse_route_key(key: str) -> tuple[str, str]:
    parts = key.split(" ", 1)
    if len(parts) == 2:
        return (parts[0].upper(), parts[1])
    return ("*", key)


def _compile_path(pattern: str) -> re.Pattern[str]:
    segments = pattern.split("/")
    parts: list[str] = []
    for i, seg in enumerate(segments):
        if i > 0:
            parts.append("/")
        if seg == "*":
            parts.append(".*?")
        elif seg.startswith(":"):
            parts.append("[^/]+")
        else:
            parts.append(re.escape(seg))
    return re.compile("^" + "".join(parts) + "$")


def _normalize_path(raw_path: str) -> str:
    idx = len(raw_path)
    for ch in ("?", "#"):
        pos = raw_path.find(ch)
        if pos >= 0 and pos < idx:
            idx = pos
    raw_path = raw_path[:idx]

    raw_path = unquote(raw_path)

    while "//" in raw_path:
        raw_path = raw_path.replace("//", "/")

    if len(raw_path) > 1 and raw_path.endswith("/"):
        raw_path = raw_path[:-1]

    if raw_path == "":
        raw_path = "/"

    return raw_path
