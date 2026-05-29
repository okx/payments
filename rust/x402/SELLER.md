# x402 Rust Seller SDK — AI Integration Guide

> This document is designed to be read by AI coding agents (Cursor, Claude Code, Copilot, etc.)
> to generate complete x402 payment integration code for Rust servers.

## What is x402?

x402 is the HTTP 402 Payment Required protocol. It lets you charge for API access per-request. When a client requests a protected endpoint without payment, the server returns HTTP 402 with payment requirements. The client signs a payment, retries the request, and gets the resource.

## Install

```toml
[dependencies]
okxweb3-app-x402-core = "0.2"
okxweb3-app-x402-axum = "0.2"
okxweb3-app-x402-evm  = "0.2"
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
tracing-subscriber = "0.3"
```

`use` paths in code are the short lib aliases: `x402_core::*`,
`x402_axum::*`, `x402_evm::*`.

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

## Facilitator URL

By default the SDK targets `https://web3.okx.com`. To point at a different facilitator
(e.g., a private staging instance), use the `with_url` constructor:

```rust
// Default — production OKX facilitator
let client = OkxHttpFacilitatorClient::new(&api_key, &secret_key, &passphrase)?;

// Custom URL
let client = OkxHttpFacilitatorClient::with_url(
    "https://your-facilitator.example.com",
    &api_key, &secret_key, &passphrase,
)?;
```

## Production deployment behind a reverse proxy

When `RoutePaymentConfig.resource` is left unset, the SDK auto-extracts
`ResourceInfo.url` from request headers — scheme from `X-Forwarded-Proto`
(falling back to the request URI's scheme), host from `X-Forwarded-Host`
or `Host`. This URL ends up in the 402 challenge sent to buyers.

Behind a reverse proxy or load balancer, request headers can be
attacker-controlled. You should pick **one** of these to avoid letting a
malicious client poison the challenge URL:

1. **Pin `resource` per route (recommended).** Set the canonical URL
   explicitly so the middleware ignores forwarded headers:

   ```rust
   RoutePaymentConfig {
       // ...
       resource: Some("https://api.example.com/data".into()),
   }
   ```

2. **Strip / overwrite inbound `X-Forwarded-*` headers at your proxy.**
   nginx / Envoy / ALB / Cloudflare all support sanitizing or replacing
   these on ingress; check your proxy's docs and only let trusted proxies
   set them.

If you do neither, an attacker who controls request headers (e.g.
sending `X-Forwarded-Host: evil.example.com`) can cause the 402
challenge to display a URL they chose, which can be used for cache /
prompt poisoning against buyers.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OKX_API_KEY` | Yes | OKX API key |
| `OKX_SECRET_KEY` | Yes | OKX secret key |
| `OKX_PASSPHRASE` | Yes | OKX API passphrase |
| `PAY_TO_ADDRESS` | Yes | Your wallet address to receive payments |

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

## Permit2 Asset Transfer Method

The `exact` scheme supports two on-chain transfer paths, selected per-route by
`AcceptConfig.extra.assetTransferMethod`:

| Method (string) | Token requirement | Buyer prerequisite |
|---|---|---|
| `"eip3009"` (default) | Token natively implements EIP-3009 `transferWithAuthorization` (e.g., USDC) | None |
| `"permit2"` | Any ERC-20 (universal fallback) | Buyer must approve `PERMIT2_ADDRESS` for the token (one-time, off-band) |

The buyer reads this field from the 402 response and signs the matching typed-data:
EIP-3009 `TransferWithAuthorization` vs Permit2 `PermitWitnessTransferFrom`. The
server SDK simply passes the value through — picking the path is the server's
responsibility, signing it correctly is the buyer's.

### Enabling Permit2 for a route

```rust
use serde_json::json;

("GET /weather".to_string(), RoutePaymentConfig {
    accepts: vec![AcceptConfig {
        scheme: "exact".into(),
        price: "$0.05".into(),
        network: "eip155:196".into(),
        pay_to: pay_to.clone(),
        max_timeout_seconds: None,
        extra: Some(HashMap::from([(
            "assetTransferMethod".to_string(),
            json!("permit2"),
        )])),
    }],
    description: "Weather data — paid via Permit2".into(),
    mime_type: "application/json".into(),
    sync_settle: Some(true),
})
```

### Buyer prerequisite — one-time PERMIT2 approve

The buyer must approve the canonical Permit2 contract to spend the chosen ERC-20
token **before** their first Permit2-mode payment. This is an on-chain ERC-20
`approve()` call, separate from the x402 protocol itself:

```solidity
// Run once per (buyer, token) pair, never again
IERC20(token).approve(PERMIT2_ADDRESS, type(uint256).max);
```

Where `PERMIT2_ADDRESS = 0x000000000022D473030F116dDEE9F6B43aC78BA3` (Uniswap
canonical, same on every EVM chain). After this one-time approve, all subsequent
Permit2 payments are signed off-chain and don't trigger additional buyer-side
approvals.

If the buyer hasn't approved Permit2 when they try to pay, the facilitator's
`settle` call reverts on chain (insufficient allowance). Buyer-side SDKs are
expected to detect this case and prompt the user to approve before signing the
Permit2 payment payload — the OKX onchainos CLI does this automatically.

### How Permit2 settles on chain

```
buyer signature → facilitator calls x402ExactPermit2Proxy.settle(...)
                    → proxy calls PERMIT2.permitWitnessTransferFrom(...)
                      → PERMIT2 verifies signature using msg.sender (= proxy)
                        as the spender
                      → PERMIT2 calls token.transferFrom(buyer, payTo, amount)
```

The proxy address `x402ExactPermit2Proxy = 0x402085c248EeA27D92E8b30b2C58ed07f9E20001`
is fixed across all EVM chains (CREATE2 vanity). The server SDK exposes it as
`x402_evm::X402_EXACT_PERMIT2_PROXY_ADDRESS` if your code needs to display or
verify it.

### When to choose Permit2 over EIP-3009

- **You're billing in USDT** on X Layer — USDT doesn't support EIP-3009, so
  Permit2 is the only path.
- **You want to accept arbitrary ERC-20s** — Permit2 works with any
  ERC-20-compliant token, no per-token integration.
- **You want a uniform code path** — once buyers approve Permit2, all future
  Permit2-mode tokens "just work" without per-token approve flows.

Choose EIP-3009 when the token natively supports it (e.g., USDC) and you want
to avoid the one-time PERMIT2 approve prerequisite for new buyers.

## Upto Scheme (pay-by-actual-usage)

The `upto` scheme is `exact`'s sibling for **metered billing**. The buyer
signs a *cap*; the server then settles for any amount up to that cap based
on actual work done. The cap is enforced on chain — the facilitator cannot
settle for more.

Use `upto` when:

- Charging for API tokens, compute time, bandwidth, or any quantity not
  knowable until after the request is served.
- You want a single signature to cover a request whose final cost depends
  on input (e.g., an LLM call whose price varies with response length).

Use `exact` when the price is known up-front (the buyer signs the exact
charge once).

### Differences vs exact + Permit2

| Dimension | exact + Permit2 | upto |
|---|---|---|
| Signed amount semantics | = actual charge (immutable) | = cap (max settle amount) |
| EIP-712 `Witness` fields | `(to, validAfter)` | `(to, facilitator, validAfter)` |
| On-chain proxy | `0x402085...0001` | `0x4020e7...0002` |
| `assetTransferMethod` | Optional (defaults to eip3009) | **Forced to "permit2"** |
| `extra.facilitatorAddress` | Not used | **Required** — buyer pins this into `witness.facilitator` |
| Server-side settle amount | Echoes signed amount | Substituted with handler-chosen actual amount |
| `settle` ABI | `(permit, owner, witness, sig)` | `(permit, settlementAmount, owner, witness, sig)` |

### Enabling upto for a route

```rust
use x402_evm::UptoEvmScheme;

let mut server = X402ResourceServer::new(facilitator_client)
    .register("eip155:196", UptoEvmScheme::new());

server.initialize().await?;

let routes = HashMap::from([(
    "GET /usage".to_string(),
    RoutePaymentConfig {
        accepts: vec![AcceptConfig {
            scheme: "upto".into(),
            // This is the CAP the buyer signs. They authorize "up to $0.10".
            price: "$0.10".into(),
            network: "eip155:196".into(),
            pay_to: pay_to.clone(),
            max_timeout_seconds: None,
            // No need to set extra.assetTransferMethod or facilitatorAddress —
            // UptoEvmScheme injects them in enhance_payment_requirements.
            extra: None,
        }],
        description: "Pay-by-actual-usage demo".into(),
        mime_type: "application/json".into(),
        sync_settle: Some(true),
    },
)]);
```

### Telling the middleware the actual settle amount

The handler decides the real charge by setting a `settlement-overrides`
response header. The middleware reads it, swaps `requirements.amount`,
and strips the header before responding to the client:

```rust
use axum::response::Response;
use x402_axum::SETTLEMENT_OVERRIDES_HEADER;

async fn usage_handler() -> Response {
    // Compute actual cost from real work
    let actual = "$0.034";

    let mut resp = (
        axum::http::StatusCode::OK,
        Json(json!({ "tokens_used": 1342, "billed": actual })),
    ).into_response();

    resp.headers_mut().insert(
        SETTLEMENT_OVERRIDES_HEADER,
        format!(r#"{{"amount":"{}"}}"#, actual).parse().unwrap(),
    );
    resp
}
```

Supported `amount` formats:

| Format | Example | Behavior |
|---|---|---|
| Atomic units | `"50000"` | Settle for 50000 atomic units of the token |
| Percent of cap | `"50%"` | Settle for cap × 0.5 (up to 2 decimal places) |
| Dollar price | `"$0.034"` | Convert via `extra.decimals` (or 6) to atomic units |
| Zero settle | `"0"` | Facilitator short-circuits — no chain tx, no charge |

If the handler doesn't set the header, the middleware settles for the full
cap (same as exact mode would).

### What the buyer signs

`upto` buyers must sign a Permit2 typed-data whose `witness.facilitator`
matches the facilitator address advertised by `getSupported`. The buyer
SDK reads `accepts.extra.facilitatorAddress` (which `UptoEvmScheme`
auto-injects) and refuses to sign if it's missing — this binds the
signature to a specific facilitator so a leaked signature can't be
relayed by anyone else.

### Buyer prerequisites

Same as exact + Permit2: the buyer must have approved the canonical
`PERMIT2_ADDRESS` for the chosen ERC-20 token once before their first
upto payment. After that, upto and exact share the same Permit2 allowance.

### See also

The reference demo at `examples/src/upto_server.rs` is a minimal,
runnable server that ties all of the above together.
