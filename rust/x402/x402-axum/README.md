# x402-axum

Axum middleware for the [x402 payment protocol](https://x402.org). Drop a
Tower `Layer` onto any axum router and gate routes behind per-request
on-chain payments — the middleware handles the 402 challenge, payment
verification, and settlement.

Mirrors `@x402/http/express` from Coinbase's x402 TypeScript SDK.

## Install

```toml
[dependencies]
x402-axum = { git = "https://github.com/okx/payments", tag = "x402-axum-v0.1.0" }
```

`x402-core` and `x402-evm` are pulled in transitively — you don't need
to depend on them directly unless you're extending the protocol.

## Quickstart

```rust,no_run
use std::collections::HashMap;
use std::sync::Arc;

use axum::{routing::get, Router};
use x402_axum::{payment_middleware, RoutePaymentConfig, RoutesConfig};
use x402_core::server::X402ResourceServer;

async fn weather() -> &'static str {
    "It's 22°C and sunny."
}

#[tokio::main]
async fn main() {
    // Build the resource server (facilitator client + signer + chain config).
    // See `x402-core` and `x402-evm` for construction details.
    let server: Arc<X402ResourceServer> = todo!();

    // Per-route pricing.
    let mut routes: RoutesConfig = HashMap::new();
    routes.insert(
        "GET /weather".into(),
        RoutePaymentConfig {
            // amount in atomic units (e.g. USDC base units = 1e6 = $1)
            amount: "10000".into(),
            description: Some("Weather forecast".into()),
            ..Default::default()
        },
    );

    let app = Router::new()
        .route("/weather", get(weather))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

A full runnable server (with X402ResourceServer wired up) lives in
[`x402/examples/src/basic_server.rs`](../examples/src/basic_server.rs):

```bash
cargo run -p x402-examples --example basic_server
```

## Hooks

Customize the request lifecycle by passing closures into the middleware
builder — `OnBeforeVerifyHook`, `OnAfterVerifyHook`,
`OnBeforeSettleHook`, `OnAfterSettleHook`, settlement-timeout recovery,
and more. See the crate-level rustdoc (`cargo doc --open -p x402-axum`)
for the full hook surface.

## Re-exports

Convenience re-exports from `x402-core::http` (`RoutesConfig`,
`RoutePaymentConfig`, hook signatures, etc.) and `x402-core::server`
(`X402ResourceServer`) so most users only need this one crate.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
