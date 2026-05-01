# x402 Rust Seller SDK — AI Integration Guide

> This document is designed to be read by AI coding agents (Cursor, Claude Code, Copilot, etc.)
> to generate complete x402 payment integration code for Rust servers.

## What is x402?

x402 is the HTTP 402 Payment Required protocol. It lets you charge for API access per-request. When a client requests a protected endpoint without payment, the server returns HTTP 402 with payment requirements. The client signs a payment, retries the request, and gets the resource.

## Install

```toml
[dependencies]
x402-core = { git = "https://github.com/okx/payments" }
x402-axum = { git = "https://github.com/okx/payments" }
x402-evm  = { git = "https://github.com/okx/payments" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
tracing-subscriber = "0.3"
```

## Complete Example (Axum)

```rust
use std::collections::HashMap;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use x402_axum::{payment_middleware, AcceptConfig, RoutePaymentConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::{ExactEvmScheme, AggrDeferredEvmScheme};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Read credentials from environment variables
    let api_key = std::env::var("OKX_API_KEY").expect("OKX_API_KEY required");
    let secret_key = std::env::var("OKX_SECRET_KEY").expect("OKX_SECRET_KEY required");
    let passphrase = std::env::var("OKX_PASSPHRASE").expect("OKX_PASSPHRASE required");
    let pay_to = std::env::var("PAY_TO_ADDRESS").expect("PAY_TO_ADDRESS required");

    // 1. Create facilitator client (default URL: https://web3.okx.com)
    //    HMAC-SHA256 signing is automatic on every request.
    let facilitator = OkxHttpFacilitatorClient::new(
        &api_key, &secret_key, &passphrase,
    ).expect("Failed to create facilitator client");
    // Or with custom URL:
    // let facilitator = OkxHttpFacilitatorClient::with_url(
    //     "https://custom-facilitator.example.com",
    //     &api_key, &secret_key, &passphrase,
    // ).expect("Failed to create facilitator client");

    // 2. Create server and register payment schemes
    let mut server = X402ResourceServer::new(facilitator)
        .register("eip155:196", ExactEvmScheme::new())
        .register("eip155:196", AggrDeferredEvmScheme::new());

    // 3. Initialize — fetches supported schemes from facilitator (required)
    server.initialize().await.expect("Failed to initialize");

    // 4. Define which routes require payment
    let routes = HashMap::from([
        ("GET /api/data".to_string(), RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "exact".into(),
                price: "$0.01".into(),
                network: "eip155:196".into(),
                pay_to: pay_to.clone(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "Protected data endpoint".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        }),
    ]);

    // 5. Build router with payment middleware
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/api/data", get(|| async { Json(json!({"data": "secret"})) }))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Server at http://localhost:3000");
    println!("  GET /health    - free");
    println!("  GET /api/data  - $0.01 USDT on X Layer");
    axum::serve(listener, app).await.unwrap();
}
```

## API Reference

### OkxHttpFacilitatorClient

```rust
use x402_core::http::OkxHttpFacilitatorClient;

// Default URL (https://web3.okx.com)
let facilitator = OkxHttpFacilitatorClient::new(
    api_key,     // OKX API key
    secret_key,  // OKX secret key (for HMAC-SHA256 signing)
    passphrase,  // OKX passphrase
)?;

// Custom URL
let facilitator = OkxHttpFacilitatorClient::with_url(
    base_url,    // e.g. "https://web3.okx.com"
    api_key,
    secret_key,
    passphrase,
)?;
```

HMAC-SHA256 signing is automatic on every Facilitator request.

### X402ResourceServer

```rust
use x402_core::server::X402ResourceServer;
use x402_evm::{ExactEvmScheme, AggrDeferredEvmScheme};

let mut server = X402ResourceServer::new(facilitator)
    .register("eip155:196", ExactEvmScheme::new())      // exact scheme on X Layer
    .register("eip155:196", AggrDeferredEvmScheme::new());   // deferred scheme on X Layer

// Required: fetch supported schemes from facilitator before use
server.initialize().await.expect("Failed to initialize");
```

### Payment Schemes

| Scheme | Struct | Description |
|--------|--------|-------------|
| `"exact"` | `ExactEvmScheme` | Standard EIP-3009 on-chain payment |
| `"aggr_deferred"` | `AggrDeferredEvmScheme` | Session key signing, OKX batches on-chain |

### RoutePaymentConfig

```rust
RoutePaymentConfig {
    accepts: Vec<AcceptConfig>,       // Payment options
    description: String,              // Resource description
    mime_type: String,                // Response MIME type
    sync_settle: Option<bool>,        // None=async, Some(true)=wait for chain confirmation
}
```

### AcceptConfig

```rust
AcceptConfig {
    scheme: String,                   // "exact" or "aggr_deferred"
    price: String,                    // Dollar amount, e.g. "$0.01", "$1.50"
    network: String,                  // CAIP-2 identifier, e.g. "eip155:196" (X Layer)
    pay_to: String,                   // Seller's wallet address (0x...)
    max_timeout_seconds: Option<u64>, // Payment timeout (default: 300s)
    extra: Option<HashMap<String, serde_json::Value>>, // Scheme-specific metadata
}
```

### Routes Configuration

Routes are defined as `HashMap<String, RoutePaymentConfig>` where keys are `"METHOD /path"`:

```rust
let routes = HashMap::from([
    ("GET /api/data".to_string(), RoutePaymentConfig { ... }),
    ("POST /api/submit".to_string(), RoutePaymentConfig { ... }),
]);
```

### payment_middleware

```rust
use x402_axum::payment_middleware;

// Basic (timeout auto-polls for 5s, then 402)
app.layer(payment_middleware(routes, server));

// Custom poll deadline
app.layer(payment_middleware_with_poll_deadline(routes, server, Duration::from_secs(10)));

// With timeout hook fallback
app.layer(payment_middleware_with_timeout_hook(routes, server, hook));

// With both timeout hook and custom poll deadline
app.layer(payment_middleware_with_timeout_hook_and_deadline(routes, server, hook, Duration::from_secs(10)));

// With dynamic payment resolver (override price/payTo per request)
app.layer(payment_middleware_with_resolver(routes, server, resolver));

// Full builder pattern with lifecycle hooks
let layer = PaymentMiddlewareBuilder::new(routes, server)
    .on_protected_request(Box::new(|ctx| Box::pin(async move {
        ProtectedRequestResult { grant_access: false, abort: false, reason: None }
    })))
    .on_before_verify(Box::new(|ctx| Box::pin(async move {
        BeforeHookResult { abort: false, reason: None }
    })))
    .on_after_verify(Box::new(|ctx| Box::pin(async move { () })))
    .on_verify_failure(Box::new(|ctx, err| Box::pin(async move { None })))
    .on_before_settle(Box::new(|ctx| Box::pin(async move {
        BeforeHookResult { abort: false, reason: None }
    })))
    .on_after_settle(Box::new(|ctx| Box::pin(async move { () })))
    .on_settle_failure(Box::new(|ctx, err| Box::pin(async move { None })))
    .on_settlement_timeout(timeout_hook)
    .poll_deadline(Duration::from_secs(10))
    .resolver(resolver_fn)
    .build();
app.layer(layer);
```

## Supported Networks

| Chain | Network ID | Token | Contract | Decimals |
|-------|-----------|-------|----------|----------|
| X Layer | `eip155:196` | USD₮0 | `0x779ded0c9e1022225f8e0630b35a9b54be713736` | 6 |

X Layer assets are pre-registered — the SDK converts dollar prices to token amounts automatically.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OKX_API_KEY` | Yes | OKX API key |
| `OKX_SECRET_KEY` | Yes | OKX secret key |
| `OKX_PASSPHRASE` | Yes | OKX API passphrase |
| `PAY_TO_ADDRESS` | Yes | Your wallet address to receive payments |
| `FACILITATOR_URL` | No | Default: `https://web3.okx.com` |

## Running

```bash
OKX_API_KEY=your-key OKX_SECRET_KEY=your-secret OKX_PASSPHRASE='your-pass' \
PAY_TO_ADDRESS=0xYourAddress cargo run
```

## Payment Flow

```
Client: GET /api/data (no payment)
  → Server: 402 + PAYMENT-REQUIRED header

Client: signs payment with wallet

Client: GET /api/data + PAYMENT-SIGNATURE header
  → Server: verify → handler → settle → 200 + data + PAYMENT-RESPONSE header
```

## Multiple Routes with Different Prices

```rust
let routes = HashMap::from([
    ("GET /api/basic".to_string(), RoutePaymentConfig {
        accepts: vec![AcceptConfig {
            scheme: "exact".into(), price: "$0.001".into(),
            network: "eip155:196".into(), pay_to: pay_to.clone(),
            max_timeout_seconds: None, extra: None,
        }],
        description: "Basic data".into(),
        mime_type: "application/json".into(),
        sync_settle: None,
    }),
    ("GET /api/premium".to_string(), RoutePaymentConfig {
        accepts: vec![AcceptConfig {
            scheme: "exact".into(), price: "$0.10".into(),
            network: "eip155:196".into(), pay_to: pay_to.clone(),
            max_timeout_seconds: None, extra: None,
        }],
        description: "Premium analytics".into(),
        mime_type: "application/json".into(),
        sync_settle: None,
    }),
]);
```

## Multiple Payment Methods Per Route

```rust
("GET /api/data".to_string(), RoutePaymentConfig {
    accepts: vec![
        AcceptConfig {
            scheme: "exact".into(), price: "$0.01".into(),
            network: "eip155:196".into(), pay_to: pay_to.clone(),
            max_timeout_seconds: None, extra: None,
        },
        AcceptConfig {
            scheme: "aggr_deferred".into(), price: "$0.01".into(),
            network: "eip155:196".into(), pay_to: pay_to.clone(),
            max_timeout_seconds: None, extra: None,
        },
    ],
    description: "Accepts both payment methods".into(),
    mime_type: "application/json".into(),
    sync_settle: None,
})
```

## Free + Paid Routes Together

Routes NOT in the `routes` HashMap are free:

```rust
let app = Router::new()
    .route("/health", get(health_handler))     // FREE — not in routes
    .route("/api/data", get(data_handler))     // PAID — in routes
    .layer(payment_middleware(routes, server));
```

## Sync vs Async Settlement

```rust
// Async (default): settle returns immediately with status="pending"
RoutePaymentConfig { ..., sync_settle: None }

// Sync: settle waits for chain confirmation, returns status="success"
RoutePaymentConfig { ..., sync_settle: Some(true) }
```
