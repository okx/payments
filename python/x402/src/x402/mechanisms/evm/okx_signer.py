"""OKX signer factory for creating EVM signers from OKX configuration."""

from __future__ import annotations

from dataclasses import dataclass

from .signer import ClientEvmSigner


@dataclass
class TEEConfig:
    """Configuration for TEE-based signing (not yet implemented)."""

    endpoint: str
    access_key: str


@dataclass
class OKXSignerConfig:
    """Configuration for OKX EVM signer.

    Provide either private_key or tee.
    """

    private_key: str | None = None
    tee: TEEConfig | None = None


def new_okx_signer(config: OKXSignerConfig) -> ClientEvmSigner:
    """Create a ClientEvmSigner from OKX configuration.

    Currently only private key signing is supported.
    TEE signing is not yet implemented.

    Args:
        config: Signer configuration with either private_key or tee.

    Returns:
        A ClientEvmSigner implementation.

    Raises:
        ValueError: If neither private_key nor tee is provided.
        NotImplementedError: If tee is provided.
    """
    if config.tee is None and not config.private_key:
        raise ValueError("must provide either tee or private_key")

    if config.tee is not None:
        raise NotImplementedError("TEE signing is not yet implemented")

    from eth_account import Account

    from .signers import EthAccountSigner

    key = config.private_key
    if key and not key.startswith("0x"):
        key = "0x" + key

    account = Account.from_key(key)
    return EthAccountSigner(account)
