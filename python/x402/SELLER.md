# x402 Python Seller SDK — AI Integration Guide

> This document is designed to be read by AI coding agents (Cursor, Claude Code, Copilot, etc.)
> to generate complete x402 payment integration code for Python servers.

## What is x402?

x402 is the HTTP 402 Payment Required protocol. It lets you charge for API access per-request. When a client requests a protected endpoint without payment, the server returns HTTP 402 with payment requirements. The client signs a payment, retries the request, and gets the resource.

## Install

```bash
pip install okxweb3-app-x402 fastapi uvicorn
```

## Complete Example (FastAPI)

```python
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
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.mechanisms.evm.deferred.server import AggrDeferredEvmScheme
from x402.server import x402ResourceServer

pay_to = os.getenv("PAY_TO_ADDRESS", "")
if not pay_to:
    print("PAY_TO_ADDRESS required")
    sys.exit(1)

# 1. Create OKX Facilitator client
facilitator = OKXFacilitatorClient(
    OKXFacilitatorConfig(
        auth=OKXAuthConfig(
            api_key=os.getenv("OKX_API_KEY", ""),
            secret_key=os.getenv("OKX_SECRET_KEY", ""),
            passphrase=os.getenv("OKX_PASSPHRASE", ""),
        ),
        base_url=os.getenv("OKX_BASE_URL", ""),
    )
)

# 2. Register payment schemes
server = x402ResourceServer(facilitator)
server.register("eip155:196", ExactEvmScheme())
server.register("eip155:196", AggrDeferredEvmScheme())

# 3. Define which routes require payment
routes = {
    "GET /api/data": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.01", network="eip155:196", pay_to=pay_to),
            PaymentOption(scheme="aggr_deferred", price="$0.01", network="eip155:196", pay_to=pay_to),
        ],
        description="Premium data endpoint",
        mime_type="application/json",
    ),
}

# 4. Create FastAPI app with payment middleware
app = FastAPI()

@app.get("/health")
async def health():
    return {"status": "ok"}

app.add_middleware(PaymentMiddlewareASGI, routes=routes, server=server)

@app.get("/api/data")
async def data():
    return {"data": "premium content", "price": "$0.01"}


if __name__ == "__main__":
    import uvicorn
    print("Server at http://localhost:3000")
    print("  GET /health    - free")
    print("  GET /api/data  - $0.01 USDT on X Layer")
    uvicorn.run(app, host="0.0.0.0", port=3000)
```

## API Reference

### OKXFacilitatorClient

```python
from x402.http import OKXFacilitatorClient, OKXFacilitatorConfig, OKXAuthConfig

facilitator = OKXFacilitatorClient(
    OKXFacilitatorConfig(
        auth=OKXAuthConfig(
            api_key="your-api-key",
            secret_key="your-secret-key",
            passphrase="your-passphrase",
        ),
        base_url="https://web3.okx.com",  # default if empty
        sync_settle=True,                  # True=sync wait for confirm, False=async
        timeout=30.0,                      # optional, default 30s
    )
)
```

HMAC-SHA256 signing is automatic on every Facilitator request.

### OKXAuthConfig

```python
from x402.http import OKXAuthConfig

auth = OKXAuthConfig(
    api_key="...",      # OKX API key (required)
    secret_key="...",   # OKX secret key for HMAC-SHA256 signing (required)
    passphrase="...",   # OKX API passphrase (required)
)
```

### RouteConfig

```python
from x402.http import PaymentOption
from x402.http.types import RouteConfig

routes = {
    "GET /api/data": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.01", network="eip155:196", pay_to="0xYourAddress"),
            PaymentOption(scheme="aggr_deferred", price="$0.01", network="eip155:196", pay_to="0xYourAddress"),
        ],
        description="Resource description",
        mime_type="application/json",
    ),
}
```

### FastAPI Middleware (ASGI)

```python
from x402.http.middleware.fastapi import PaymentMiddlewareASGI

app.add_middleware(PaymentMiddlewareASGI, routes=routes, server=server)
```

### Flask Middleware

```python
from x402.http.middleware.flask import payment_middleware

app = Flask(__name__)
payment_middleware(app, routes=routes, server=server)
```

### Payment Schemes

```python
from x402.mechanisms.evm.exact.server import ExactEvmScheme
from x402.mechanisms.evm.deferred.server import AggrDeferredEvmScheme

server = x402ResourceServer(facilitator)
server.register("eip155:196", ExactEvmScheme())
server.register("eip155:196", AggrDeferredEvmScheme())
```

| Scheme            | Class                      | Description                               |
| ----------------- | -------------------------- | ----------------------------------------- |
| `"exact"`         | `ExactEvmScheme()`         | Standard EIP-3009 on-chain payment        |
| `"aggr_deferred"` | `AggrDeferredEvmScheme()`  | Session key signing, OKX batches on-chain |

## Supported Networks

Pre-configured networks with default assets.

| Chain        | Network ID     | Token | Contract                                     | Decimals | Transfer Method |
| ------------ | -------------- | ----- | -------------------------------------------- | -------- | --------------- |
| X Layer      | `eip155:196`   | USDT  | `0x779Ded0c9e1022225f8E0630b35a9b54bE713736` | 6        | EIP-3009        |
| Base         | `eip155:8453`  | USDC  | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` | 6        | EIP-3009        |
| Base Sepolia | `eip155:84532` | USDC  | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` | 6        | EIP-3009        |
| MegaETH      | `eip155:4326`  | USDM  | `0xFAfDdbb3FC7688494971a79cc65DCa3EF82079E7` | 18       | Permit2         |
| Monad        | `eip155:143`   | USDC  | `0x754704Bc059F8C67012fEd69BC8A327a5aafb603` | 6        | EIP-3009        |
| Mezo Testnet | `eip155:31611` | mUSD  | `0x118917a40FAF1CD7a13dB0Ef56C86De7973Ac503` | 18       | Permit2         |
| Stable       | `eip155:988`   | USDT0 | `0x779Ded0c9e1022225f8E0630b35a9b54bE713736` | 6        | EIP-3009        |

## Environment Variables

| Variable         | Required | Description                                           |
| ---------------- | -------- | ----------------------------------------------------- |
| `OKX_API_KEY`    | Yes      | OKX API key                                           |
| `OKX_SECRET_KEY` | Yes      | OKX secret key                                        |
| `OKX_PASSPHRASE` | Yes      | OKX API passphrase                                    |
| `PAY_TO_ADDRESS` | Yes      | Your wallet address to receive payments               |
| `OKX_BASE_URL`   | No       | Facilitator URL (default: `https://www.web3.okx.com`) |

## Running

```bash
OKX_API_KEY=your-key OKX_SECRET_KEY=your-secret OKX_PASSPHRASE='your-pass' \
OKX_BASE_URL=web3.okx.com \
PAY_TO_ADDRESS=0xYourAddress python main.py
```

Or with uvicorn directly:

```bash
OKX_API_KEY=your-key OKX_SECRET_KEY=your-secret OKX_PASSPHRASE='your-pass' \
PAY_TO_ADDRESS=0xYourAddress uvicorn main:app --port 3000
```

## Payment Flow

```
Client: GET /api/data (no payment)
  → Server: HTTP 402 + PAYMENT-REQUIRED header (base64-encoded PaymentRequired JSON)

Client: signs payment with wallet

Client: GET /api/data + PAYMENT-SIGNATURE header (base64-encoded PaymentPayload)
  → Server: verify → handler → settle → HTTP 200 + data + PAYMENT-RESPONSE header
```

## Multiple Routes with Different Prices

```python
routes = {
    "GET /api/basic": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.001", network="eip155:196", pay_to=pay_to),
            PaymentOption(scheme="aggr_deferred", price="$0.001", network="eip155:196", pay_to=pay_to),
        ],
        description="Basic data",
        mime_type="application/json",
    ),
    "GET /api/premium": RouteConfig(
        accepts=[
            PaymentOption(scheme="exact", price="$0.10", network="eip155:196", pay_to=pay_to),
            PaymentOption(scheme="aggr_deferred", price="$0.10", network="eip155:196", pay_to=pay_to),
        ],
        description="Premium analytics",
        mime_type="application/json",
    ),
}
```

## Multiple Payment Methods Per Route

```python
"GET /api/data": RouteConfig(
    accepts=[
        PaymentOption(scheme="exact", price="$0.01", network="eip155:196", pay_to=pay_to),
        PaymentOption(scheme="aggr_deferred", price="$0.01", network="eip155:196", pay_to=pay_to),
    ],
    description="Accepts both exact and deferred payments",
    mime_type="application/json",
),
```

## Free + Paid Routes Together

Routes NOT in the middleware config are free:

```python
@app.get("/health")
async def health():            # FREE — not in routes config
    return {"status": "ok"}

app.add_middleware(PaymentMiddlewareASGI, routes=routes, server=server)

@app.get("/api/data")
async def data():              # PAID — matched by routes config
    return {"data": "premium content"}
```

## Sync vs Async Settlement

Default is **sync** (`sync_settle=True`).

```python
# Sync (default): settle waits for on-chain confirmation
facilitator = OKXFacilitatorClient(
    OKXFacilitatorConfig(
        auth=auth_config,
        base_url=base_url,
    )
)

# Async: settle returns immediately, settles in background
facilitator = OKXFacilitatorClient(
    OKXFacilitatorConfig(
        auth=auth_config,
        base_url=base_url,
        sync_settle=False,
    )
)
```
