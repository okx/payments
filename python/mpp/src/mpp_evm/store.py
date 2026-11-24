"""File-backed store implementing pympp's Store protocol.

with cross-language channel state sharing.

Usage:
    store = FileStore("/path/to/channels")
    await store.put("0xabc...", {"channel_id": "0xabc...", ...})
    state = await store.get("0xabc...")
"""

from __future__ import annotations

import copy
import json
import os
from typing import Any


class FileStore:
    """JSON file-backed key-value store.

    Each key maps to ``{directory}/{key}.json``.
    Cross-language compatible file-based key-value store.
    """

    def __init__(self, directory: str) -> None:
        self._dir = directory
        os.makedirs(directory, exist_ok=True)

    def _path(self, key: str) -> str:
        path = os.path.join(self._dir, f"{key}.json")
        # Confine to the store directory: a crafted key (e.g. "../../etc/passwd"
        # or an absolute path) must not escape self._dir.
        real_dir = os.path.realpath(self._dir)
        real_path = os.path.realpath(path)
        if real_path != real_dir and not real_path.startswith(real_dir + os.sep):
            raise ValueError(f"invalid store key: {key!r}")
        return path

    async def get(self, key: str) -> Any | None:
        path = self._path(key)
        if not os.path.exists(path):
            return None
        with open(path) as f:
            return json.load(f)

    async def put(self, key: str, value: Any) -> None:
        path = self._path(key)
        tmp = path + ".tmp"
        with open(tmp, "w") as f:
            json.dump(value, f, indent=2)
        os.replace(tmp, path)

    async def delete(self, key: str) -> None:
        path = self._path(key)
        if os.path.exists(path):
            os.remove(path)

    async def put_if_absent(self, key: str, value: Any) -> bool:
        path = self._path(key)
        if os.path.exists(path):
            return False
        await self.put(key, value)
        return True


class InMemoryChannelStore:
    """In-process dict-backed channel store (non-persistent).

    The default store used by ``SessionIntent`` when none is supplied, mirroring
    the Go SDK's ``NewMemoryStore`` default. Values are deep-copied on put/get so
    callers cannot mutate stored state by reference — matching ``FileStore``'s
    JSON round-trip isolation.

    Not durable and not shared across processes/workers; for cross-language or
    multi-worker deployments use ``FileStore`` or another shared backend.
    """

    def __init__(self) -> None:
        self._data: dict[str, Any] = {}

    async def get(self, key: str) -> Any | None:
        value = self._data.get(key)
        return copy.deepcopy(value) if value is not None else None

    async def put(self, key: str, value: Any) -> None:
        self._data[key] = copy.deepcopy(value)

    async def delete(self, key: str) -> None:
        self._data.pop(key, None)

    async def put_if_absent(self, key: str, value: Any) -> bool:
        if key in self._data:
            return False
        await self.put(key, value)
        return True
