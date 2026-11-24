"""Unit tests for X Layer asset naming and exact-Decimal money conversion."""

from __future__ import annotations

from x402.mechanisms.evm.constants import NETWORK_CONFIGS
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.mechanisms.evm.utils import parse_money_to_string


class TestXLayerAssetName:
    def test_x_layer_default_asset_name(self):
        config = NETWORK_CONFIGS["eip155:196"]
        assert config["default_asset"]["name"] == "USD₮0"


class TestParseMoneyToString:
    def test_strips_dollar_sign(self):
        assert parse_money_to_string("$1.50") == "1.50"

    def test_strips_currency_suffix(self):
        assert parse_money_to_string("2.25 USDC") == "2.25"
        assert parse_money_to_string("3.00 usd") == "3.00"

    def test_passthrough_numbers(self):
        assert parse_money_to_string(1.5) == "1.5"
        assert parse_money_to_string(2) == "2"


class TestDefaultMoneyConversion:
    def test_dollar_amount_uses_exact_decimal(self):
        scheme = ExactEvmScheme()
        result = scheme.parse_price("$1.50", "eip155:196")
        assert result.amount == "1500000"
        assert result.asset == "0x779ded0c9e1022225f8e0630b35a9b54be713736"
        assert result.extra["name"] == "USD₮0"

    def test_small_fraction_has_no_float_drift(self):
        scheme = ExactEvmScheme()
        # int(2.01 * 10**6) truncates to 2009999 under binary float; Decimal keeps it exact.
        result = scheme.parse_price("$2.01", "eip155:196")
        assert result.amount == "2010000"
