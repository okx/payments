"""OKX x402 demo server — Flask (sync) version for testing timeout recovery."""

import os
import sys

from flask import Flask, jsonify

from x402.http import (
    OKXAuthConfig,
    OKXFacilitatorClientSync,
    OKXFacilitatorConfig,
    PaymentOption,
)
from x402.http.middleware.flask import payment_middleware
from x402.http.types import RouteConfig
from x402.mechanisms.evm.deferred.server import AggrDeferredEvmScheme
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.server import x402ResourceServerSync

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

# OKX facilitator client (sync)
facilitator = OKXFacilitatorClientSync(
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
server = x402ResourceServerSync(facilitator)
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
app = Flask(__name__)
payment_middleware(app, routes=routes, server=server)


@app.route("/health")
def health():
    return jsonify({"status": "ok"})


@app.route("/resource/sync")
def resource_sync():
    return jsonify({
        "message": "Payment successful! Here is your premium X Layer data (sync).",
        "network": "eip155:196",
        "settle_mode": "sync",
    })


@app.route("/resource/async")
def resource_async():
    return jsonify({
        "message": "Payment successful! Here is your premium X Layer data (async).",
        "network": "eip155:196",
        "settle_mode": "async",
    })


if __name__ == "__main__":
    port = int(os.getenv("PORT", "4002"))
    print(f"Flask seller server listening on :{port}")
    app.run(host="0.0.0.0", port=port, debug=False)
