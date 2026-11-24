"""OKX API authentication provider for HMAC-SHA256 request signing."""

from __future__ import annotations

import base64
import hashlib
import hmac
from dataclasses import dataclass
from datetime import datetime, timezone


@dataclass
class OKXAuthConfig:
    """OKX API credentials for request signing."""

    api_key: str
    secret_key: str
    passphrase: str


class OKXAuthProvider:
    """Generates per-request HMAC-SHA256 authentication headers for OKX APIs."""

    def __init__(self, config: OKXAuthConfig) -> None:
        self._api_key = config.api_key
        self._secret_key = config.secret_key
        self._passphrase = config.passphrase

    def compute_signature(self, timestamp: str, method: str, path: str, body: str) -> str:
        """Compute HMAC-SHA256 signature for an OKX API request.

        Args:
            timestamp: ISO UTC timestamp (e.g. '2025-03-30T12:00:00.000Z').
            method: HTTP method (e.g. 'GET', 'POST').
            path: Request path (e.g. '/api/v6/x402/verify').
            body: Request body string (empty string for GET).

        Returns:
            Base64-encoded HMAC-SHA256 signature.
        """
        prehash = timestamp + method + path + body
        mac = hmac.new(
            self._secret_key.encode("utf-8"),
            prehash.encode("utf-8"),
            hashlib.sha256,
        )
        return base64.b64encode(mac.digest()).decode("utf-8")

    def create_headers(self, method: str, path: str, body: str = "") -> dict[str, str]:
        """Create OKX authentication headers for a request.

        Args:
            method: HTTP method (e.g. 'GET', 'POST').
            path: Request path (e.g. '/api/v6/x402/verify').
            body: Request body string (empty string for GET).

        Returns:
            Dict with OK-ACCESS-KEY, OK-ACCESS-SIGN, OK-ACCESS-TIMESTAMP,
            OK-ACCESS-PASSPHRASE headers.
        """
        now = datetime.now(timezone.utc)
        timestamp = now.strftime("%Y-%m-%dT%H:%M:%S.") + f"{now.microsecond // 1000:03d}Z"
        sign = self.compute_signature(timestamp, method, path, body)

        return {
            "OK-ACCESS-KEY": self._api_key,
            "OK-ACCESS-SIGN": sign,
            "OK-ACCESS-TIMESTAMP": timestamp,
            "OK-ACCESS-PASSPHRASE": self._passphrase,
        }
