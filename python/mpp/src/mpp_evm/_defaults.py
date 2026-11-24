"""EVM payment method constants and defaults."""

from __future__ import annotations

X_LAYER_CHAIN_ID: int = 196

DEFAULT_ESCROW_CONTRACT: str = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b"

DEFAULT_DOMAIN_NAME: str = "EVM Payment Channel"
DEFAULT_DOMAIN_VERSION: str = "1"

PROOF_DOMAIN_NAME: str = "MPP"
PROOF_DOMAIN_VERSION: str = "1"

METHOD_NAME_EVM: str = "evm"

DEFAULT_EXPIRES_MINUTES: int = 5

MAX_SPLITS: int = 10

ACTION_OPEN: str = "open"
ACTION_TOP_UP: str = "topUp"
ACTION_VOUCHER: str = "voucher"
ACTION_CLOSE: str = "close"
ACTION_SETTLE: str = "settle"

STATUS_OPEN: str = "open"
STATUS_CLOSED: str = "closed"

SA_API_BASE_PATH: str = "/api/v6/pay/mpp"
