"""Tests for SA API client HMAC-SHA256 auth and request handling."""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
from unittest.mock import AsyncMock, patch

import pytest

from mpp_evm.errors import ChannelNotFoundError, InternalSAError
from mpp_evm.saclient.client import OKXSAClient
from mpp_evm.saclient.types import (
    ChargeReceipt,
    ChargeSettleRequest,
    ChargeTransactionPayload,
    ChargeVerifyHashRequest,
    ChargeHashPayload,
    ChallengeEcho,
    Eip3009Authorization,
    SessionCloseRequest,
    SessionClosePayload,
    SessionOpenRequest,
    SessionOpenPayload,
    SessionReceipt,
    SessionSettleRequest,
    SessionSettlePayload,
    SessionStatus,
    SessionTopUpRequest,
    SessionTopUpPayload,
)


class TestHMACAuth:
    """Tests for the HMAC-SHA256 signature computation."""

    def test_sign_computation(self) -> None:
        """Verify HMAC-SHA256 signature matches expected value."""
        client = OKXSAClient(
            base_url="https://sa.example.com",
            api_key="test-key",
            secret_key="test-secret",
            passphrase="test-pass",
        )

        timestamp = "2026-05-04T12:00:00Z"
        method = "POST"
        path = "/api/v6/pay/mpp/charge/settle"
        body = '{"payload":{"type":"transaction"}}'

        sig = client._sign(timestamp, method, path, body)

        # Verify independently
        prehash = timestamp + method + path + body
        expected_mac = hmac.new(b"test-secret", prehash.encode(), hashlib.sha256)
        expected_sig = base64.b64encode(expected_mac.digest()).decode()

        assert sig == expected_sig

    def test_sign_empty_body_for_get(self) -> None:
        """GET requests sign with empty body string."""
        client = OKXSAClient(
            base_url="https://sa.example.com",
            api_key="k",
            secret_key="s",
            passphrase="p",
        )

        timestamp = "2026-01-01T00:00:00Z"
        path = "/api/v6/pay/mpp/session/status?channelId=0xabc"

        sig = client._sign(timestamp, "GET", path, "")

        prehash = timestamp + "GET" + path + ""
        expected_mac = hmac.new(b"s", prehash.encode(), hashlib.sha256)
        expected_sig = base64.b64encode(expected_mac.digest()).decode()

        assert sig == expected_sig

    def test_auth_headers_structure(self) -> None:
        """Verify all required auth headers are set."""
        client = OKXSAClient(
            base_url="https://sa.example.com",
            api_key="my-key",
            secret_key="my-secret",
            passphrase="my-pass",
        )

        headers = client._auth_headers("POST", "/api/v6/pay/mpp/charge/settle", "{}")

        assert headers["OK-ACCESS-KEY"] == "my-key"
        assert headers["OK-ACCESS-PASSPHRASE"] == "my-pass"
        assert headers["Content-Type"] == "application/json"
        assert "OK-ACCESS-SIGN" in headers
        assert "OK-ACCESS-TIMESTAMP" in headers
        # Timestamp should be RFC3339 UTC
        ts = headers["OK-ACCESS-TIMESTAMP"]
        assert ts.endswith("Z")
        assert "T" in ts

    def test_sign_with_query_params(self) -> None:
        """Path for signing includes query string for GET requests."""
        client = OKXSAClient(
            base_url="https://sa.example.com",
            api_key="k",
            secret_key="secret123",
            passphrase="p",
        )

        path_with_query = "/api/v6/pay/mpp/session/status?channelId=0xdef"
        timestamp = "2026-03-15T08:30:00Z"

        sig = client._sign(timestamp, "GET", path_with_query, "")

        prehash = timestamp + "GET" + path_with_query
        expected = base64.b64encode(
            hmac.new(b"secret123", prehash.encode(), hashlib.sha256).digest()
        ).decode()
        assert sig == expected


class TestResponseHandling:
    """Tests for SA response parsing and error handling."""

    @pytest.fixture
    def client(self) -> OKXSAClient:
        return OKXSAClient(
            base_url="https://sa.example.com",
            api_key="k",
            secret_key="s",
            passphrase="p",
        )

    def test_handle_success_response(self, client: OKXSAClient) -> None:
        """Successful response returns data dict."""

        class FakeResp:
            status_code = 200
            text = ""

            def json(self):
                return {"code": 0, "msg": "success", "data": {"reference": "0xabc"}}

        result = client._handle_response(FakeResp())
        assert result == {"reference": "0xabc"}

    def test_handle_non_200_status(self, client: OKXSAClient) -> None:
        """Non-200 HTTP status raises InternalSAError."""

        class FakeResp:
            status_code = 500
            text = "Internal Server Error"

        with pytest.raises(InternalSAError) as exc_info:
            client._handle_response(FakeResp())
        assert "HTTP 500" in exc_info.value.detail

    def test_handle_sa_business_error(self, client: OKXSAClient) -> None:
        """Non-zero SA code raises mapped error."""

        class FakeResp:
            status_code = 200
            text = ""

            def json(self):
                # 70010 = channel_not_found per the [Pay] MPP EVM API spec table
                return {"code": 70010, "msg": "channel not found", "data": None}

        with pytest.raises(ChannelNotFoundError) as exc_info:
            client._handle_response(FakeResp())
        assert exc_info.value.detail == "channel not found"

    def test_handle_json_decode_error(self, client: OKXSAClient) -> None:
        """Invalid JSON raises InternalSAError."""

        class FakeResp:
            status_code = 200
            text = "not json"

            def json(self):
                raise ValueError("decode error")

        with pytest.raises(InternalSAError) as exc_info:
            client._handle_response(FakeResp())
        assert "decode" in exc_info.value.detail.lower()

    def test_handle_string_code(self, client: OKXSAClient) -> None:
        """SA response with string code (e.g. "0") is handled."""

        class FakeResp:
            status_code = 200
            text = ""

            def json(self):
                return {"code": "0", "msg": "ok", "data": {"status": "open"}}

        result = client._handle_response(FakeResp())
        assert result == {"status": "open"}

    def test_handle_empty_data(self, client: OKXSAClient) -> None:
        """Null/missing data field returns empty dict."""

        class FakeResp:
            status_code = 200
            text = ""

            def json(self):
                return {"code": 0, "msg": "ok", "data": None}

        result = client._handle_response(FakeResp())
        assert result == {}


class TestRequestTypes:
    """Tests for SA request/response Pydantic model serialization."""

    def test_charge_settle_request_serialization(self) -> None:
        req = ChargeSettleRequest(
            challenge=ChallengeEcho(
                id="ch-1",
                realm="test.com",
                method="evm",
                intent="charge",
                request="base64data",
            ),
            payload=ChargeTransactionPayload(
                type="transaction",
                authorization=Eip3009Authorization(
                    type="eip-3009",
                    from_address="0x1234",
                    to="0x5678",
                    value="1000000",
                    valid_after="0",
                    valid_before="9999999999",
                    nonce="0xabc",
                    signature="0xsig",
                ),
            ),
            source="did:pkh:eip155:196:0x1234",
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["challenge"]["id"] == "ch-1"
        assert data["payload"]["type"] == "transaction"
        assert data["payload"]["authorization"]["from"] == "0x1234"
        assert data["source"] == "did:pkh:eip155:196:0x1234"

    def test_charge_verify_hash_request(self) -> None:
        req = ChargeVerifyHashRequest(
            payload=ChargeHashPayload(type="hash", hash="0xdeadbeef"),
            source="did:pkh:eip155:196:0xpayer",
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["payload"]["hash"] == "0xdeadbeef"
        assert data["payload"]["type"] == "hash"
        assert "challenge" not in data

    def test_session_open_request(self) -> None:
        req = SessionOpenRequest(
            payload=SessionOpenPayload(
                action="open",
                type="transaction",
                channel_id="0xchannel",
                salt="0xsalt",
                authorized_signer="0xsigner",
                authorization=Eip3009Authorization(
                    type="eip-3009",
                    from_address="0xfrom",
                    to="0xescrow",
                    value="5000000",
                    valid_after="0",
                    valid_before="99999",
                    nonce="0xnonce",
                ),
                signature="0xeip3009sig",
            ),
            source="did:pkh:eip155:196:0xfrom",
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["payload"]["channelId"] == "0xchannel"
        assert data["payload"]["action"] == "open"
        assert data["payload"]["authorizedSigner"] == "0xsigner"

    def test_session_settle_request(self) -> None:
        req = SessionSettleRequest(
            payload=SessionSettlePayload(
                action="settle",
                channel_id="0xch",
                cumulative_amount="500000",
                voucher_signature="0xvsig",
                payee_signature="0xpsig",
                nonce="12345",
                deadline="99999999",
            )
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["payload"]["channelId"] == "0xch"
        assert data["payload"]["cumulativeAmount"] == "500000"
        assert data["payload"]["voucherSignature"] == "0xvsig"
        assert data["payload"]["payeeSignature"] == "0xpsig"

    def test_charge_receipt_deserialization(self) -> None:
        data = {
            "method": "evm",
            "reference": "0xtxhash",
            "status": "success",
            "timestamp": "2026-05-04T12:00:00Z",
            "chainId": 196,
            "challengeId": "ch-1",
            "externalId": "order-123",
        }
        receipt = ChargeReceipt.model_validate(data)
        assert receipt.method == "evm"
        assert receipt.reference == "0xtxhash"
        assert receipt.chain_id == 196
        assert receipt.challenge_id == "ch-1"
        assert receipt.external_id == "order-123"

    def test_session_receipt_deserialization(self) -> None:
        data = {
            "method": "evm",
            "intent": "session",
            "status": "open",
            "timestamp": "2026-05-04T12:00:00Z",
            "channelId": "0xchannel",
            "chainId": 196,
            "reference": "0xtx",
            "deposit": "10000000",
        }
        receipt = SessionReceipt.model_validate(data)
        assert receipt.channel_id == "0xchannel"
        assert receipt.deposit == "10000000"
        assert receipt.chain_id == 196

    def test_session_status_deserialization(self) -> None:
        data = {
            "channelId": "0xch",
            "payer": "0xpayer",
            "payee": "0xpayee",
            "token": "0xtoken",
            "deposit": "1000000",
            "cumulativeAmount": "500000",
            "settledOnChain": "200000",
            "remainingBalance": "500000",
            "sessionStatus": "OPEN",
        }
        status = SessionStatus.model_validate(data)
        assert status.channel_id == "0xch"
        assert status.session_status == "OPEN"
        assert status.remaining_balance == "500000"

    def test_session_close_request(self) -> None:
        req = SessionCloseRequest(
            payload=SessionClosePayload(
                action="close",
                channel_id="0xch",
                cumulative_amount="1000000",
                voucher_signature="0xvsig",
                payee_signature="0xpsig",
                nonce="123",
                deadline="999",
            )
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["payload"]["action"] == "close"

    def test_session_top_up_request(self) -> None:
        req = SessionTopUpRequest(
            payload=SessionTopUpPayload(
                action="topUp",
                type="transaction",
                channel_id="0xch",
                additional_deposit="5000000",
                top_up_salt="0xsalt",
                authorization=Eip3009Authorization(
                    type="eip-3009",
                    from_address="0xfrom",
                    to="0xescrow",
                    value="5000000",
                    valid_after="0",
                    valid_before="99999",
                    nonce="0xnonce",
                ),
                signature="0xsig",
            ),
        )
        data = req.model_dump(by_alias=True, exclude_none=True)
        assert data["payload"]["additionalDeposit"] == "5000000"
        assert data["payload"]["topUpSalt"] == "0xsalt"
