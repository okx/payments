"""Tests for EVM-specific Pydantic models."""

from __future__ import annotations

import pytest

from mpp_evm.types import (
    ChargeRequest,
    ChargeSplit,
    ClosePayload,
    EVMMethodDetails,
    EVMSessionMethodDetails,
    OpenAuthorization,
    OpenPayload,
    SessionSplit,
    TopUpPayload,
    VoucherPayload,
)


class TestEVMMethodDetails:
    def test_parse_from_dict(self) -> None:
        data = {
            "chainId": 196,
            "feePayer": True,
            "memo": "test payment",
            "splits": [{"amount": "5000", "recipient": "0xabc", "memo": "fee"}],
        }
        details = EVMMethodDetails.model_validate(data)
        assert details.chain_id == 196
        assert details.is_fee_payer is True
        assert details.memo == "test payment"
        assert len(details.splits) == 1
        assert details.splits[0].amount == "5000"

    def test_fee_payer_defaults_false(self) -> None:
        details = EVMMethodDetails.model_validate({})
        assert details.is_fee_payer is False

    def test_serialization_uses_aliases(self) -> None:
        details = EVMMethodDetails(chain_id=196, fee_payer=True)
        data = details.model_dump(by_alias=True, exclude_none=True)
        assert "chainId" in data
        assert "feePayer" in data
        assert data["chainId"] == 196


class TestEVMSessionMethodDetails:
    def test_parse_full(self) -> None:
        data = {
            "escrowContract": "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b",
            "channelId": "0xchannel",
            "minVoucherDelta": "30",
            "chainId": 196,
            "feePayer": False,
            "splits": [{"recipient": "0xabc", "bps": 500}],
        }
        details = EVMSessionMethodDetails.model_validate(data)
        assert details.escrow_contract == "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b"
        assert details.channel_id == "0xchannel"
        assert details.min_voucher_delta == "30"
        assert details.is_fee_payer is False
        assert details.splits[0].bps == 500


class TestOpenPayload:
    def test_validate_transaction_mode_valid(self) -> None:
        payload = OpenPayload(
            type="transaction",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xdepositsig",
            authorization=OpenAuthorization(from_address="0xpayer", value="1000000"),
            voucher_signature="0xvouchersig",
        )
        assert payload.validate_fields() is None

    def test_validate_transaction_mode_missing_auth(self) -> None:
        payload = OpenPayload(
            type="transaction",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xsig",
            voucher_signature="0xvsig",
        )
        err = payload.validate_fields()
        assert err is not None
        assert "authorization" in err

    def test_validate_hash_mode_valid(self) -> None:
        payload = OpenPayload(
            type="hash",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xvouchersig",
            hash="0xtxhash",
        )
        assert payload.validate_fields() is None

    def test_validate_hash_mode_missing_hash(self) -> None:
        payload = OpenPayload(
            type="hash",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xsig",
        )
        err = payload.validate_fields()
        assert err is not None
        assert "hash" in err

    def test_validate_invalid_type(self) -> None:
        payload = OpenPayload(
            type="invalid",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xsig",
        )
        err = payload.validate_fields()
        assert err is not None
        assert "invalid" in err

    def test_effective_deposit_from_authorization(self) -> None:
        payload = OpenPayload(
            type="transaction",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xsig",
            authorization=OpenAuthorization(from_address="0x", value="5000000"),
            voucher_signature="0xvsig",
        )
        assert payload.effective_deposit == "5000000"

    def test_effective_deposit_from_deposit_field(self) -> None:
        payload = OpenPayload(
            type="hash",
            channel_id="0xch",
            salt="0xsalt",
            cumulative_amount="0",
            signature="0xsig",
            hash="0xtx",
            deposit="3000000",
        )
        assert payload.effective_deposit == "3000000"


class TestTopUpPayload:
    def test_validate_transaction_mode(self) -> None:
        payload = TopUpPayload(
            type="transaction",
            channel_id="0xch",
            additional_deposit="5000000",
            authorization=OpenAuthorization(from_address="0x", value="5000000"),
            signature="0xsig",
            top_up_salt="0xsalt",
        )
        assert payload.validate_fields() is None

    def test_validate_missing_channel_id(self) -> None:
        payload = TopUpPayload(
            type="transaction",
            channel_id="",
            additional_deposit="100",
            signature="0xsig",
            top_up_salt="0xsalt",
            authorization=OpenAuthorization(from_address="0x", value="100"),
        )
        err = payload.validate_fields()
        assert err is not None
        assert "channelId" in err


class TestVoucherPayload:
    def test_validate_valid(self) -> None:
        payload = VoucherPayload(
            channel_id="0xch",
            cumulative_amount="250000",
            signature="0xsig",
        )
        assert payload.validate_fields() is None

    def test_validate_missing_signature(self) -> None:
        payload = VoucherPayload(
            channel_id="0xch",
            cumulative_amount="250000",
            signature="",
        )
        err = payload.validate_fields()
        assert err is not None
        assert "signature" in err


class TestClosePayload:
    def test_validate_valid(self) -> None:
        payload = ClosePayload(
            channel_id="0xch",
            cumulative_amount="500000",
            signature="0xsig",
        )
        assert payload.validate_fields() is None

    def test_validate_missing_amount(self) -> None:
        payload = ClosePayload(
            channel_id="0xch",
            cumulative_amount="",
            signature="0xsig",
        )
        err = payload.validate_fields()
        assert err is not None
        assert "cumulativeAmount" in err


class TestChargeRequest:
    def test_parse_full_request(self) -> None:
        data = {
            "amount": "1000000",
            "currency": "0xA8CE8aee21bC2A48a5EF670afCc9274C7bbbC035",
            "recipient": "0x742d35Cc6634c0532925a3b844bC9e7595F8fE00",
            "description": "API access",
            "externalId": "order-123",
            "methodDetails": {
                "chainId": 196,
                "feePayer": True,
                "splits": [{"amount": "50000", "recipient": "0xfee", "memo": "fee"}],
            },
        }
        req = ChargeRequest.model_validate(data)
        assert req.amount == "1000000"
        assert req.currency == "0xA8CE8aee21bC2A48a5EF670afCc9274C7bbbC035"
        assert req.method_details is not None
        assert req.method_details.chain_id == 196
        assert req.method_details.is_fee_payer is True
        assert len(req.method_details.splits) == 1
