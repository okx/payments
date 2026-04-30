# x402 Rust Seller SDK

Rust implementation of the [x402 payment protocol](https://www.x402.org/) for resource servers (sellers). Translated from the [Coinbase x402 TypeScript SDK](https://github.com/coinbase/x402), adapted for OKX Facilitator and X Layer.

## Architecture

```
x402-core     ← @x402/core          Core types, server logic, facilitator client, HMAC signing
x402-axum     ← @x402/http/express  Axum middleware (Tower Layer/Service)
x402-evm      ← @x402/mechanisms/evm  EVM mechanism, X Layer asset pre-registration
```

## Quick Start

```rust
use std::collections::HashMap;
use axum::{routing::get, Json, Router};
use serde_json::json;

use x402_axum::{payment_middleware, AcceptConfig, RoutePaymentConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::{ExactEvmScheme, AggrDeferredEvmScheme};

#[tokio::main]
async fn main() {
    let pay_to = "0xYourSellerAddress";

    // 1. Configure OKX Facilitator (default URL: https://web3.okx.com)
    let facilitator = OkxHttpFacilitatorClient::new(
        "your-api-key",
        "your-secret-key",
        "your-passphrase",
    ).expect("Failed to create facilitator client");

    // 2. Create server and register schemes
    let mut server = X402ResourceServer::new(facilitator)
        .register("eip155:196", ExactEvmScheme::new())
        .register("eip155:196", AggrDeferredEvmScheme::new());

    // 3. Initialize — fetches supported schemes from facilitator (required)
    server.initialize().await.expect("Failed to initialize");

    // 4. Define payment-protected routes
    let routes = HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![
                AcceptConfig {
                    scheme: "exact".into(),
                    price: "$0.001".into(),
                    network: "eip155:196".into(),
                    pay_to: pay_to.into(),
                    max_timeout_seconds: None,
                    extra: None,
                },
                AcceptConfig {
                    scheme: "aggr_deferred".into(),
                    price: "$0.001".into(),
                    network: "eip155:196".into(),
                    pay_to: pay_to.into(),
                    max_timeout_seconds: None,
                    extra: None,
                },
            ],
            description: "Weather data".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        },
    )]);

    // 5. Build Axum app with payment middleware
    let app = Router::new()
        .route("/weather", get(|| async { Json(json!({"weather": "sunny"})) }))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4021").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Configuration

Seller developers provide three OKX credentials. The SDK automatically signs every Facilitator API call with HMAC-SHA256:

| Parameter | Description |
|-----------|-------------|
| `api_key` | OKX API key |
| `secret_key` | OKX secret key (used for HMAC-SHA256 signing) |
| `passphrase` | OKX API passphrase |

Headers added automatically to every Facilitator request:
- `OK-ACCESS-KEY`
- `OK-ACCESS-SIGN` — `Base64(HMAC-SHA256(secret_key, timestamp + METHOD + path + body))`
- `OK-ACCESS-TIMESTAMP`
- `OK-ACCESS-PASSPHRASE`

## Supported Networks

| Chain | Network ID | Token | Contract | Decimals |
|-------|-----------|-------|----------|----------|
| X Layer | `eip155:196` | USD₮0 | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6 |

X Layer assets are pre-registered — no manual configuration needed. For other tokens (e.g., USDG), use `register_money_parser()` or specify an `AssetAmount` directly in the price field.

## Payment Schemes

| Scheme | Description |
|--------|-------------|
| `exact` | Standard EIP-3009 payment. Seller can submit on-chain. |
| `aggr_deferred` | OKX extension. Session key signing, TEE-backed. Seller cannot submit on-chain. |

## Payment Flow

```
Client GET /weather (no payment)
    ↓
Server returns 402 + PAYMENT-REQUIRED header
    ↓
Client creates payment signature
    ↓
Client GET /weather + PAYMENT-SIGNATURE header
    ↓
Middleware → Facilitator POST /verify
    ↓
Middleware → Route handler (200 OK)
    ↓
Middleware → Facilitator POST /settle
    ↓
Response + PAYMENT-RESPONSE header
```

## OKX Facilitator API

Default URL: `https://web3.okx.com`

Base path: `/api/v6/pay/x402`

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/supported` | GET | Query supported schemes and networks |
| `/verify` | POST | Verify payment authorization (no on-chain tx) |
| `/settle` | POST | Submit for on-chain settlement |
| `/settle/status?txHash=` | GET | Query settlement status (used for timeout polling) |

### OKX Extensions

- **`syncSettle`** (settle request) — `true` to wait for on-chain confirmation
- **`status`** (settle response) — `"pending"` / `"success"` / `"timeout"`
- **`transaction`** (settle response) — tx hash (empty for aggr_deferred)

## Development

```bash
# Check compilation
cargo check

# Run tests
cargo test --workspace

# Run examples (uses default facilitator URL https://web3.okx.com)
OKX_API_KEY=... OKX_SECRET_KEY=... OKX_PASSPHRASE=... PAY_TO_ADDRESS=0x... \
  cargo run --example basic_server

# With custom facilitator URL
FACILITATOR_URL=http://your-test-facilitator.com \
OKX_API_KEY=... OKX_SECRET_KEY=... OKX_PASSPHRASE=... PAY_TO_ADDRESS=0x... \
  cargo run --example basic_server
```

## Crate Structure

```
rust/
├── x402-core/          # Core types, server, facilitator client, HMAC signing
│   ├── types/          # PaymentRequirements, PaymentPayload, VerifyResponse, etc.
│   ├── server/         # X402ResourceServer (register, initialize, verify, settle)
│   ├── facilitator/    # FacilitatorClient trait
│   ├── http/           # OkxHttpFacilitatorClient, HMAC signing, header encode/decode
│   ├── schemas/        # Validation logic
│   └── utils/          # Base64, pattern matching
├── x402-axum/          # Axum Tower middleware
│   ├── middleware.rs   # PaymentLayer + PaymentMiddleware
│   └── adapter.rs      # Request info extraction
├── x402-evm/           # EVM mechanism
│   ├── constants.rs    # X Layer default assets
│   ├── exact/          # ExactEvmScheme
│   └── aggr_deferred/  # AggrDeferredEvmScheme (OKX extension)
└── examples/
    ├── basic_server.rs
    ├── custom_config.rs
    └── test_connectivity.rs
```

## License

Apache-2.0
