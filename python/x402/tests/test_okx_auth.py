"""Unit tests for x402.http.okx_auth — OKX HMAC-SHA256 authentication."""

from __future__ import annotations

import base64
import hashlib
import hmac
import re

from x402.http.okx_auth import OKXAuthConfig, OKXAuthProvider


def _make_provider() -> OKXAuthProvider:
    return OKXAuthProvider(OKXAuthConfig(api_key="key", secret_key="secret", passphrase="pass"))


def _expected_signature(secret: str, prehash: str) -> str:
    mac = hmac.new(secret.encode(), prehash.encode(), hashlib.sha256)
    return base64.b64encode(mac.digest()).decode()


class TestComputeSignature:
    def test_signature_format(self):
        provider = _make_provider()
        ts = "2026-03-30T12:00:00.000Z"
        method = "POST"
        path = "/api/v6/pay/x402/verify"
        body = '{"test":"data"}'

        sig = provider.compute_signature(ts, method, path, body)
        expected = _expected_signature("secret", ts + method + path + body)
        assert sig == expected

    def test_empty_body(self):
        provider = _make_provider()
        ts = "2026-03-30T12:00:00.000Z"

        sig = provider.compute_signature(ts, "GET", "/api/v6/pay/x402/supported", "")
        expected = _expected_signature("secret", ts + "GET" + "/api/v6/pay/x402/supported")
        assert sig == expected

    def test_different_secrets_produce_different_signatures(self):
        p1 = OKXAuthProvider(OKXAuthConfig(api_key="k", secret_key="secret1", passphrase="p"))
        p2 = OKXAuthProvider(OKXAuthConfig(api_key="k", secret_key="secret2", passphrase="p"))
        ts = "2026-03-30T12:00:00.000Z"
        s1 = p1.compute_signature(ts, "POST", "/path", "body")
        s2 = p2.compute_signature(ts, "POST", "/path", "body")
        assert s1 != s2


class TestCreateHeaders:
    def test_headers_contain_all_keys(self):
        provider = _make_provider()
        headers = provider.create_headers("POST", "/api/v6/pay/x402/verify", '{"data":1}')

        required_keys = {"OK-ACCESS-KEY", "OK-ACCESS-SIGN", "OK-ACCESS-TIMESTAMP", "OK-ACCESS-PASSPHRASE"}
        assert set(headers.keys()) == required_keys
        for v in headers.values():
            assert v, "header value should be non-empty"

    def test_headers_key_and_passphrase_values(self):
        provider = _make_provider()
        headers = provider.create_headers("GET", "/path")
        assert headers["OK-ACCESS-KEY"] == "key"
        assert headers["OK-ACCESS-PASSPHRASE"] == "pass"

    def test_timestamp_format(self):
        provider = _make_provider()
        headers = provider.create_headers("GET", "/path")
        ts = headers["OK-ACCESS-TIMESTAMP"]
        assert re.match(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$", ts)

    def test_signature_is_verifiable(self):
        provider = _make_provider()
        headers = provider.create_headers("POST", "/api/v6/pay/x402/settle", '{"a":1}')
        ts = headers["OK-ACCESS-TIMESTAMP"]
        expected = _expected_signature("secret", ts + "POST" + "/api/v6/pay/x402/settle" + '{"a":1}')
        assert headers["OK-ACCESS-SIGN"] == expected
