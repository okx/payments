"""Shared test fixtures."""

from __future__ import annotations

import pytest


@pytest.fixture
def sa_credentials() -> dict[str, str]:
    """Test SA API credentials."""
    return {
        "base_url": "https://sa.test.example.com",
        "api_key": "test-api-key-123",
        "secret_key": "test-secret-key-456",
        "passphrase": "test-passphrase",
    }
