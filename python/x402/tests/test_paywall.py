"""Unit tests for paywall display formatting (amount precision + token name)."""

from __future__ import annotations

import pytest

from x402.http.paywall import _format_amount, _get_display_currency
from x402.http.x402_http_server_base import x402HTTPServerBase
from x402.schemas.payments import PaymentRequired, PaymentRequirements


def _payment_required(extra: dict | None = None) -> PaymentRequired:
    return PaymentRequired(
        accepts=[
            PaymentRequirements(
                scheme="exact",
                network="eip155:196",
                asset="0xtok",
                amount="1500000",
                pay_to="0xmerchant",
                max_timeout_seconds=300,
                extra=extra or {},
            )
        ]
    )


class TestFormatAmount:
    @pytest.mark.parametrize(
        "amount,expected",
        [
            (1.5, "1.5"),
            (1.0, "1"),
            (0.0, "0"),
            (0.000001, "0.000001"),
            (1.234567, "1.234567"),
            (2.25, "2.25"),
        ],
    )
    def test_up_to_six_decimals_no_trailing_zeros(self, amount, expected):
        assert _format_amount(amount) == expected
        assert x402HTTPServerBase._format_display_amount(amount) == expected


class TestDisplayCurrency:
    def test_uses_token_name_from_extra(self):
        pr = _payment_required(extra={"name": "USD₮0"})
        assert _get_display_currency(pr) == "USD₮0"
        assert x402HTTPServerBase._get_display_currency(pr) == "USD₮0"

    def test_falls_back_when_name_missing(self):
        pr = _payment_required(extra={})
        assert _get_display_currency(pr) == "tokens"
        assert x402HTTPServerBase._get_display_currency(pr) == "tokens"
