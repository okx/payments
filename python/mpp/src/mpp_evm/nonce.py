"""NonceProvider protocol and UUID-based implementation.

"""

from __future__ import annotations

import uuid
from typing import Protocol, runtime_checkable


@runtime_checkable
class NonceProvider(Protocol):
    """Protocol for authorization nonce allocation.

    """

    async def allocate(self, payee: str, channel_id: str) -> int:
        """Allocate a unique nonce for a payee + channel pair.

        Returns a large integer suitable for uint256.
        """
        ...


class UuidNonceProvider:
    """Generates UUID v4 as 128-bit big integer nonce.

    """

    async def allocate(self, payee: str, channel_id: str) -> int:
        return uuid.uuid4().int
