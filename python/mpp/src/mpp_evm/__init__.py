"""mpp-evm: EVM payment method for MPP (Micropayment Protocol).

This package provides:
- SA API client (OKXSAClient) with HMAC-SHA256 authentication
- EVM-specific types (Pydantic models for charge/session payloads)
- SA error code → MPP error mapping
- Constants for X Layer chain, escrow contract, EIP-712 domains
"""

# Patch pympp to support session respond() hook (pympp#139 shim).
# DELETE this import once pympp ships native support.
import mpp_evm._patch_session_respond  # noqa: F401

from mpp_evm._defaults import (
    ACTION_CLOSE,
    ACTION_OPEN,
    ACTION_SETTLE,
    ACTION_TOP_UP,
    ACTION_VOUCHER,
    DEFAULT_DOMAIN_NAME,
    DEFAULT_DOMAIN_VERSION,
    DEFAULT_ESCROW_CONTRACT,
    METHOD_NAME_EVM,
    STATUS_CLOSED,
    STATUS_OPEN,
    X_LAYER_CHAIN_ID,
)
from mpp_evm.errors import (
    AmountExceedsDepositError,
    BadRequestError,
    ChannelClosedError,
    ChannelNotFoundError,
    DeltaTooSmallError,
    EvmPaymentError,
    InsufficientBalanceError,
    InternalSAError,
    InvalidChallengeError,
    InvalidSignatureError,
    InvalidSplitError,
    SignerMismatchError,
    map_sa_error,
)
from mpp_evm.charge.intent import ChargeIntent
from mpp_evm.charge.method import EvmChargeMethod
from mpp_evm.method import EvmMethod
from mpp_evm.saclient.client import OKXSAClient, SAClient
from mpp_evm.store import FileStore
from mpp_evm.adapters import MppAdapter, MppRouteConfig

__all__ = [
    # Method + Intent
    "EvmMethod",
    "EvmChargeMethod",
    "ChargeIntent",
    # Client
    "OKXSAClient",
    "SAClient",
    # Store
    "FileStore",
    # Errors
    "EvmPaymentError",
    "AmountExceedsDepositError",
    "BadRequestError",
    "ChannelClosedError",
    "ChannelNotFoundError",
    "DeltaTooSmallError",
    "InsufficientBalanceError",
    "InternalSAError",
    "InvalidChallengeError",
    "InvalidSignatureError",
    "InvalidSplitError",
    "SignerMismatchError",
    "map_sa_error",
    # Constants
    "X_LAYER_CHAIN_ID",
    "DEFAULT_ESCROW_CONTRACT",
    "DEFAULT_DOMAIN_NAME",
    "DEFAULT_DOMAIN_VERSION",
    "METHOD_NAME_EVM",
    "ACTION_OPEN",
    "ACTION_TOP_UP",
    "ACTION_VOUCHER",
    "ACTION_CLOSE",
    "ACTION_SETTLE",
    "STATUS_OPEN",
    "STATUS_CLOSED",
    # Adapters
    "MppAdapter",
    "MppRouteConfig",
]
