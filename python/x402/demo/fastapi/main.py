"""OKX x402 demo server — Python equivalent of go/x402/demo/okx-server."""

import os
import sys

from fastapi import FastAPI

from x402.http import (
    OKXAuthConfig,
    OKXFacilitatorClient,
    OKXFacilitatorConfig,
    PaymentOption,
)
from x402.http.middleware.fastapi import PaymentMiddlewareASGI
from x402.http.types import RouteConfig
from x402.mechanisms.evm.deferred.server import AggrDeferredEvmScheme
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.server import x402ResourceServer

# Config
PAY_TO = os.getenv("PAY_TO_ADDRESS", "")
PAY_TO_ASYNC = os.getenv("PAY_TO_ADDRESS_ASYNC", "") or PAY_TO
OKX_BASE_URL = os.getenv("OKX_BASE_URL", "")

if not PAY_TO:
    print("PAY_TO_ADDRESS is required")
    sys.exit(1)
if not OKX_BASE_URL:
    print("OKX_BASE_URL is required")
    sys.exit(1)

# OKX facilitator client
facilitator = OKXFacilitatorClient(
    OKXFacilitatorConfig(
        auth=OKXAuthConfig(
            api_key=os.getenv("OKX_API_KEY", ""),
            secret_key=os.getenv("OKX_SECRET_KEY", ""),
            passphrase=os.getenv("OKX_PASSPHRASE", ""),
        ),
        base_url=OKX_BASE_URL,
        sync_settle=True,
    )
)

# Server with EVM schemes
server = x402ResourceServer(facilitator)
server.register("eip155:196", ExactEvmScheme())
server.register("eip155:196", AggrDeferredEvmScheme())

# Routes 
routes = {
    "GET /resource/sync": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.00001", network="eip155:196", pay_to=PAY_TO, max_timeout_seconds=300),
            PaymentOption(scheme="aggr_deferred", price="$0.00001", network="eip155:196", pay_to=PAY_TO, max_timeout_seconds=300),
        ],
        description="Premium X Layer data (sync)",
        mime_type="application/json",
    ),
    "GET /resource/async": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.00001", network="eip155:196", pay_to=PAY_TO_ASYNC, max_timeout_seconds=300),
            PaymentOption(scheme="aggr_deferred", price="$0.00001", network="eip155:196", pay_to=PAY_TO_ASYNC, max_timeout_seconds=300),
        ],
        description="Premium X Layer data (async)",
        mime_type="application/json",
    ),
}

# App
app = FastAPI()
app.add_middleware(PaymentMiddlewareASGI, routes=routes, server=server)


@app.get("/health")
async def health():
    return {"status": "ok"}


@app.get("/resource/sync")
async def resource_sync():
    return {"message": "Payment successful! Here is your premium X Layer data (sync).", "network": "eip155:196", "settle_mode": "sync"}


@app.get("/resource/async")
async def resource_async():
    return {"message": "Payment successful! Here is your premium X Layer data (async).", "network": "eip155:196", "settle_mode": "async"}


if __name__ == "__main__":
    import uvicorn

    port = int(os.getenv("PORT", "4001"))
    print(f"Seller server listening on :{port}")
    uvicorn.run(app, host="0.0.0.0", port=port)
