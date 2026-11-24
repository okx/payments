"""PaymentRouter demo — dual protocol (MPP + x402) on the same endpoints.

Mirrors go/demo/paymentrouter/gin/main.go.

Usage:
    uvicorn main:app --port 8082

Endpoints:
    GET /free          — no payment required
    GET /api/onetime   — dual protocol (MPP charge + x402 exact)
    GET /api/batch     — dual protocol (MPP session + x402 aggr_deferred)

Config via env vars:
    PRIVATE_KEY           — Payee hex private key (required for MPP session)
    PAY_TO_ADDRESS        — Payment recipient
    MPP_SECRET_KEY        — Server challenge secret (default: "demo-secret-key")
    ESCROW_CONTRACT       — Escrow contract address (default: DEFAULT_ESCROW_CONTRACT)
    OKX_BASE_URL          — OKX API base URL
    OKX_API_KEY           — OKX API key
    OKX_SECRET_KEY        — OKX API secret
    OKX_PASSPHRASE        — OKX API passphrase
    CHANNEL_STORE_DIR     — Channel state dir (default: "./mpp-data/channels")
    PORT                  — Listen port (default: 8082)
"""

from __future__ import annotations

import os
import sys

from fastapi import Depends, FastAPI

from mpp_evm import (
    X_LAYER_CHAIN_ID,
    DEFAULT_ESCROW_CONTRACT,
    FileStore,
    MppAdapter,
    MppRouteConfig,
)
from mpp_evm.charge.intent import ChargeIntent
from mpp_evm.method import EvmMethod
from mpp_evm.saclient.client import OKXSAClient
from mpp_evm.session.intent import SessionIntent
from mpp_evm.signer import PrivateKeySigner
from mpp.server.mpp import Mpp

from x402.http import (
    OKXAuthConfig,
    OKXFacilitatorClient,
    OKXFacilitatorConfig,
    PaymentOption,
)
from x402.http.types import RouteConfig as X402RouteConfig
from x402.mechanisms.evm.deferred.server import AggrDeferredEvmScheme
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.http import x402HTTPResourceServer
from x402.server import x402ResourceServer
from x402.adapters import X402Adapter

from paymentrouter import PaymentGate, RouteConfig
from paymentrouter.fastapi.middleware import register_management_handler


def _env(key: str, default: str = "") -> str:
    return os.environ.get(key, default)


TOKEN_ADDRESS = "0x779ded0c9e1022225f8e0630b35a9b54be713736"
DECIMALS = 6


def create_app() -> FastAPI:
    """Create dual-protocol demo server."""
    pay_to = _env("PAY_TO_ADDRESS", "0xYourRecipientAddress")
    escrow_contract = _env("ESCROW_CONTRACT", DEFAULT_ESCROW_CONTRACT)

    # --- MPP setup ---
    sa_client = OKXSAClient(
        base_url=_env("OKX_BASE_URL", "https://web3.okx.com"),
        api_key=_env("OKX_API_KEY", ""),
        secret_key=_env("OKX_SECRET_KEY", ""),
        passphrase=_env("OKX_PASSPHRASE", ""),
    )

    mpp_secret_key = _env("MPP_SECRET_KEY", "demo-secret-key")

    charge_intent = ChargeIntent(
        sa_client=sa_client,
        chain_id=X_LAYER_CHAIN_ID,
        recipient=pay_to,
        fee_payer=True,
    )

    channel_dir = _env("CHANNEL_STORE_DIR", "./mpp-data/channels")
    store = FileStore(channel_dir)

    payee_signer = None
    session_intent = None
    pk = _env("PRIVATE_KEY")
    if pk:
        payee_signer = PrivateKeySigner.from_hex(pk)
        session_intent = SessionIntent(
            sa_client=sa_client,
            recipient=pay_to,
            signer=payee_signer,
            store=store,
            chain_id=X_LAYER_CHAIN_ID,
            escrow_contract=escrow_contract,
            per_request_cost=10,
            min_voucher_delta=30,
            fee_payer=True,
        )

    intents: dict = {"charge": charge_intent}
    if session_intent:
        intents["session"] = session_intent

    method = EvmMethod(intents=intents)
    method.currency = TOKEN_ADDRESS
    method.recipient = pay_to
    method.decimals = DECIMALS
    method.chain_id = X_LAYER_CHAIN_ID

    mpp = Mpp(method=method, realm="mpp", secret_key=mpp_secret_key)

    # --- x402 setup ---
    facilitator = OKXFacilitatorClient(
        OKXFacilitatorConfig(
            auth=OKXAuthConfig(
                api_key=_env("OKX_API_KEY", "mock-api-key"),
                secret_key=_env("OKX_SECRET_KEY", "mock-secret-key"),
                passphrase=_env("OKX_PASSPHRASE", "mock-passphrase"),
            ),
            base_url=_env("OKX_BASE_URL", "https://www.web3.okx.com"),
            sync_settle=True,
        )
    )

    x402_resource = x402ResourceServer(facilitator)
    x402_resource.register("eip155:196", ExactEvmScheme())
    x402_resource.register("eip155:196", AggrDeferredEvmScheme())
    x402_server = x402HTTPResourceServer(x402_resource, routes={})

    # --- Real adapters ---
    mpp_adapter = MppAdapter(mpp)
    x402_adapter = X402Adapter(x402_server)

    # --- PaymentGate ---
    paid = PaymentGate(
        protocols=[mpp_adapter, x402_adapter],
        on_error=lambda err, phase, protocol: print(f"[{protocol}] {phase}: {err}"),
    )

    onetime_cfg: RouteConfig = {
        "mpp": MppRouteConfig(
            intent="charge",
            amount="0.00001",
            currency=TOKEN_ADDRESS,
            decimals=DECIMALS,
            description="One-time payment",
        ),
        "x402": X402RouteConfig(
            accepts=[
                PaymentOption(
                    scheme="exact",
                    price="$0.00001",
                    network="eip155:196",
                    pay_to=pay_to,
                    max_timeout_seconds=300,
                ),
            ],
            description="One-time payment",
            mime_type="application/json",
        ),
    }

    batch_cfg: RouteConfig = {
        "mpp": MppRouteConfig(
            intent="session",
            amount="0.00001",
            currency=TOKEN_ADDRESS,
            decimals=DECIMALS,
            description="Batch session",
            unit_type="request",
            suggested_deposit="60",
        ),
        "x402": X402RouteConfig(
            accepts=[
                PaymentOption(
                    scheme="aggr_deferred",
                    price="$0.00001",
                    network="eip155:196",
                    pay_to=pay_to,
                    max_timeout_seconds=300,
                ),
            ],
            description="Batch session",
            mime_type="application/json",
        ),
    }

    app = FastAPI(title="PaymentRouter Demo — Dual Protocol")
    register_management_handler(app)

    @app.get("/free")
    async def free_endpoint():
        return {"message": "This endpoint is free!"}

    @app.get("/api/onetime", dependencies=[Depends(paid.for_route(onetime_cfg))])
    async def onetime():
        return {"message": "One-time payment received!"}

    @app.get("/api/batch", dependencies=[Depends(paid.for_route(batch_cfg))])
    async def batch():
        return {"message": "Batch request served!"}

    print("PaymentRouter demo")
    print("  GET /free          — no payment")
    print("  GET /api/onetime   — dual (MPP charge + x402 exact)")
    print("  GET /api/batch     — dual (MPP session + x402 aggr_deferred)")

    return app


app = create_app() if _env("PRIVATE_KEY") else None


def main() -> None:
    import uvicorn

    port = int(_env("PORT", "8082"))
    print(f"PaymentRouter demo listening on :{port}")
    uvicorn.run(create_app(), host="0.0.0.0", port=port)


if __name__ == "__main__":
    main()
