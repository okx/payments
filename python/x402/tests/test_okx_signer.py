"""Unit tests for x402.mechanisms.evm.okx_signer — OKX signer factory."""

from __future__ import annotations

import pytest

from x402.mechanisms.evm.okx_signer import OKXSignerConfig, TEEConfig, new_okx_signer


# Well-known Hardhat test private key — do NOT use in production
_TEST_PRIVATE_KEY = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
_EXPECTED_ADDRESS = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"


class TestNewOKXSigner:
    def test_private_key_signer(self):
        signer = new_okx_signer(OKXSignerConfig(private_key=_TEST_PRIVATE_KEY))
        assert signer.address == _EXPECTED_ADDRESS

    def test_private_key_without_0x_prefix(self):
        key_no_prefix = _TEST_PRIVATE_KEY[2:]
        signer = new_okx_signer(OKXSignerConfig(private_key=key_no_prefix))
        assert signer.address == _EXPECTED_ADDRESS

    def test_no_config(self):
        with pytest.raises(ValueError, match="must provide either tee or private_key"):
            new_okx_signer(OKXSignerConfig())

    def test_tee_not_implemented(self):
        with pytest.raises(NotImplementedError, match="TEE signing is not yet implemented"):
            new_okx_signer(OKXSignerConfig(
                tee=TEEConfig(endpoint="https://tee.example.com", access_key="ak-123"),
            ))

    def test_signer_has_sign_typed_data(self):
        signer = new_okx_signer(OKXSignerConfig(private_key=_TEST_PRIVATE_KEY))
        assert hasattr(signer, "sign_typed_data")
        assert callable(signer.sign_typed_data)
