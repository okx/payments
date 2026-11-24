"""MPP EVM Demo Server — mirrors go/demo/mpp/gin/main.go.

Uses the @mpp.pay() decorator for verify-or-challenge flow.

Endpoints:
  GET  /free                 — Free endpoint (no payment)
  GET  /charge/tx/primary    — Charge: 0.00001 USDT, tx mode, feePayer=true
  GET  /charge/tx/split      — Charge: 0.00003 USDT (10+20 split), tx mode, feePayer=true
  GET  /charge/hash/primary  — Charge: 0.00001 USDT, hash mode, feePayer=false
  GET  /charge/hash/split    — Charge: 0.00003 USDT (10+20 split), hash mode, feePayer=false
  GET  /session/tx           — Session: 0.00001 USDT per req, feePayer=true
  GET  /session/hash         — Session: 0.00001 USDT per req, feePayer=false

Config via env vars (mirrors Go demo):
  PRIVATE_KEY           — Payee hex private key (required)
  PAY_TO_ADDRESS        — Payment recipient (defaults to signer address)
  PAY_TO_ADDRESS_SPLIT  — Split recipient (defaults to PAY_TO_ADDRESS)
  MPP_SECRET_KEY        — Server challenge secret (default: "demo-secret-key")
  ESCROW_CONTRACT       — Escrow contract address (default: DEFAULT_ESCROW_CONTRACT)
  OKX_BASE_URL          — SA API base URL (default: "https://web3.okx.com")
  OKX_API_KEY           — SA API key
  OKX_SECRET_KEY        — SA API secret key
  OKX_PASSPHRASE        — SA API passphrase
  CHANNEL_STORE_DIR     — Channel state dir (default: "./mpp-data/channels")
  ADDR                  — Listen address (default: ":8080")
"""

from __future__ import annotations

import os
import signal
import sys

from fastapi import FastAPI, Request
from mpp import Credential, Receipt
from mpp.server.mpp import Mpp

from mpp_evm import (
    X_LAYER_CHAIN_ID,
    DEFAULT_ESCROW_CONTRACT,
    FileStore,
)
from mpp_evm.charge.intent import ChargeIntent
from mpp_evm.method import EvmMethod
from mpp_evm.saclient.client import OKXSAClient
from mpp_evm.session.intent import SessionIntent
from mpp_evm.signer import PrivateKeySigner


# ---------------------------------------------------------------------------
# Configuration (mirrors Go envOrDefault pattern)
# ---------------------------------------------------------------------------

def _env(key: str, default: str = "") -> str:
    return os.environ.get(key, default)


# USDT on X Layer
TOKEN_ADDRESS = "0x779ded0c9e1022225f8e0630b35a9b54be713736"
DECIMALS = 6

# Amounts (human-readable for decorator; 0.00001 USDT with 6 decimals = 10 base units)
AMOUNT_PRIMARY = "0.00001"
AMOUNT_SPLIT_TOTAL = "0.00003"
AMOUNT_SESSION_PER_REQUEST = "0.00001"


# ---------------------------------------------------------------------------
# App factory
# ---------------------------------------------------------------------------


def create_app() -> FastAPI:
    """Create and configure the FastAPI demo server."""
    # --- Payee signer (from PRIVATE_KEY env var) ---
    private_key_hex = _env("PRIVATE_KEY")
    if not private_key_hex:
        print("FATAL: PRIVATE_KEY environment variable is required", file=sys.stderr)
        sys.exit(1)

    signer = PrivateKeySigner.from_hex(private_key_hex)
    pay_to_address = _env("PAY_TO_ADDRESS", signer.address)
    pay_to_address_split = _env("PAY_TO_ADDRESS_SPLIT", pay_to_address)
    mpp_secret_key = _env("MPP_SECRET_KEY", "demo-secret-key")
    escrow_contract = _env("ESCROW_CONTRACT", DEFAULT_ESCROW_CONTRACT)

    sa_client = OKXSAClient(
        base_url=_env("OKX_BASE_URL", "https://web3.okx.com"),
        api_key=_env("OKX_API_KEY", ""),
        secret_key=_env("OKX_SECRET_KEY", ""),
        passphrase=_env("OKX_PASSPHRASE", ""),
    )

    # --- Channel store (file-backed, compatible with Go mpp-tools) ---
    channel_dir = _env("CHANNEL_STORE_DIR", "./mpp-data/channels")
    store = FileStore(channel_dir)
    print(f"Channel store: {channel_dir}")

    # --- Charge methods ---
    # Each request = one EIP-3009 authorization, settled via SA API.
    charge_intent_tx = ChargeIntent(
        sa_client=sa_client,
        chain_id=X_LAYER_CHAIN_ID,
        recipient=pay_to_address,
        fee_payer=True,
    )
    charge_intent_hash = ChargeIntent(
        sa_client=sa_client,
        chain_id=X_LAYER_CHAIN_ID,
        recipient=pay_to_address,
        fee_payer=False,
    )

    # --- Session methods ---
    # Client opens a payment channel, then sends vouchers authorizing cumulative spend.
    # Server deducts perRequestCost (10 base units) from the voucher balance on each request.
    print(f"Escrow contract: {escrow_contract}")

    session_intent_tx = SessionIntent(
        sa_client=sa_client,
        recipient=pay_to_address,
        signer=signer,
        store=store,
        chain_id=X_LAYER_CHAIN_ID,
        escrow_contract=escrow_contract,
        per_request_cost=10,
        min_voucher_delta=30,
        fee_payer=True,
    )
    session_intent_hash = SessionIntent(
        sa_client=sa_client,
        recipient=pay_to_address,
        signer=signer,
        store=store,
        chain_id=X_LAYER_CHAIN_ID,
        escrow_contract=escrow_contract,
        per_request_cost=10,
        min_voucher_delta=30,
        fee_payer=False,
    )

    session_intent_tx_other = SessionIntent(
        sa_client=sa_client,
        recipient=pay_to_address,
        signer=signer,
        store=store,
        chain_id=X_LAYER_CHAIN_ID,
        escrow_contract=escrow_contract,
        per_request_cost=15,
        min_voucher_delta=30,
        fee_payer=True,
    )

    # --- EVM methods (one per fee_payer mode, with both charge + session intents) ---
    method_tx = EvmMethod(intents={"charge": charge_intent_tx, "session": session_intent_tx})
    method_tx.currency = TOKEN_ADDRESS  # type: ignore[attr-defined]
    method_tx.recipient = pay_to_address  # type: ignore[attr-defined]
    method_tx.decimals = DECIMALS  # type: ignore[attr-defined]
    method_tx.chain_id = X_LAYER_CHAIN_ID  # type: ignore[attr-defined]

    method_hash = EvmMethod(intents={"charge": charge_intent_hash, "session": session_intent_hash})
    method_hash.currency = TOKEN_ADDRESS  # type: ignore[attr-defined]
    method_hash.recipient = pay_to_address  # type: ignore[attr-defined]
    method_hash.decimals = DECIMALS  # type: ignore[attr-defined]
    method_hash.chain_id = X_LAYER_CHAIN_ID  # type: ignore[attr-defined]

    method_tx_other = EvmMethod(intents={"session": session_intent_tx_other})
    method_tx_other.currency = TOKEN_ADDRESS  # type: ignore[attr-defined]
    method_tx_other.recipient = pay_to_address  # type: ignore[attr-defined]
    method_tx_other.decimals = DECIMALS  # type: ignore[attr-defined]
    method_tx_other.chain_id = X_LAYER_CHAIN_ID  # type: ignore[attr-defined]

    # --- Mpp server instances ---
    cfg_realm = "mpp"
    mpp_tx = Mpp(method=method_tx, realm=cfg_realm, secret_key=mpp_secret_key)
    mpp_hash = Mpp(method=method_hash, realm=cfg_realm, secret_key=mpp_secret_key)
    mpp_tx_other = Mpp(method=method_tx_other, realm=cfg_realm, secret_key=mpp_secret_key)

    app = FastAPI(title="MPP EVM Demo Server")

    # ------------------------------------------------------------------
    # Free endpoint
    # ------------------------------------------------------------------

    @app.get("/free")
    async def free_endpoint() -> dict:
        return {"message": "This endpoint is free!"}

    # ------------------------------------------------------------------
    # Charge endpoints
    # ------------------------------------------------------------------

    @app.get("/charge/tx/primary")
    @mpp_tx.pay(amount=AMOUNT_PRIMARY, intent="charge", description="Charge tx/primary: 0.00001 USDT")
    async def charge_tx_primary(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Charge payment received!", "receipt": _receipt_to_dict(receipt)}

    @app.get("/charge/tx/split")
    @mpp_tx.pay(amount=AMOUNT_SPLIT_TOTAL, intent="charge", description="Charge tx/split: 0.00003 USDT (10 primary + 20 split)")
    async def charge_tx_split(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Charge payment received!", "receipt": _receipt_to_dict(receipt)}

    @app.get("/charge/hash/primary")
    @mpp_hash.pay(amount=AMOUNT_PRIMARY, intent="charge", description="Charge hash/primary: 0.00001 USDT")
    async def charge_hash_primary(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Charge payment received!", "receipt": _receipt_to_dict(receipt)}

    @app.get("/charge/hash/split")
    @mpp_hash.pay(amount=AMOUNT_SPLIT_TOTAL, intent="charge", description="Charge hash/split: 0.00003 USDT (10 primary + 20 split)")
    async def charge_hash_split(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Charge payment received!", "receipt": _receipt_to_dict(receipt)}

    # ------------------------------------------------------------------
    # Session endpoints
    # ------------------------------------------------------------------

    @app.get("/session/tx")
    @mpp_tx.pay(amount=AMOUNT_SESSION_PER_REQUEST, intent="session", description="Session tx: 0.00001 USDT per request", unit_type="request", suggested_deposit="60")
    async def session_tx(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Session request served!", "receipt": _receipt_to_dict(receipt)}

    @app.get("/session/tx-other")
    @mpp_tx_other.pay(amount="0.000015", intent="session", description="Session tx-other: 0.000015 USDT per request (cost=15)", unit_type="request", suggested_deposit="60")
    async def session_tx_other(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Session request served!", "receipt": _receipt_to_dict(receipt)}

    @app.get("/session/hash")
    @mpp_hash.pay(amount=AMOUNT_SESSION_PER_REQUEST, intent="session", description="Session hash: 0.00001 USDT per request", unit_type="request", suggested_deposit="60")
    async def session_hash(request: Request, credential: Credential, receipt: Receipt) -> dict:
        return {"message": "Session request served!", "receipt": _receipt_to_dict(receipt)}

    return app


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _receipt_to_dict(receipt: object) -> dict:
    """Convert a Receipt to a JSON-safe dict."""
    if hasattr(receipt, "model_dump"):
        return receipt.model_dump()
    if isinstance(receipt, dict):
        return receipt
    return {
        "reference": getattr(receipt, "reference", ""),
        "method": getattr(receipt, "method", ""),
        "status": getattr(receipt, "status", ""),
        "external_id": getattr(receipt, "external_id", None),
    }


# ---------------------------------------------------------------------------
# Entrypoint
# ---------------------------------------------------------------------------

app = create_app() if _env("PRIVATE_KEY") else None


def main() -> None:
    """Run the demo server."""
    import uvicorn

    addr = _env("ADDR", ":8080")
    host = "0.0.0.0"
    port = 8080
    if addr.startswith(":"):
        port = int(addr[1:])
    elif ":" in addr:
        host, port_str = addr.rsplit(":", 1)
        port = int(port_str)

    application = create_app()

    print(f"Demo server listening on {addr}")
    print(f"  GET {addr}/free                — no payment")
    print(f"  GET {addr}/charge/tx/primary   — charge, tx mode")
    print(f"  GET {addr}/charge/tx/split     — charge with splits, tx mode")
    print(f"  GET {addr}/charge/hash/primary — charge, hash mode")
    print(f"  GET {addr}/charge/hash/split   — charge with splits, hash mode")
    print(f"  GET {addr}/session/tx          — session, feePayer=true")
    print(f"  GET {addr}/session/hash        — session, feePayer=false")

    uvicorn.run(application, host=host, port=port)


if __name__ == "__main__":
    main()
