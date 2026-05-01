//! Minimal Axum server with x402 payment middleware.
//!
//! Mirrors: Coinbase quickstart-for-sellers example.
//!
//! Run: `cargo run --example basic_server`
//!
//! Requires environment variables:
//!   OKX_API_KEY, OKX_SECRET_KEY, OKX_PASSPHRASE, PAY_TO_ADDRESS

use std::collections::HashMap;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use x402_axum::{payment_middleware, AcceptConfig, RoutePaymentConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_core::types::AssetAmount;
use x402_evm::{AggrDeferredEvmScheme, ExactEvmScheme};

#[tokio::main]
async fn main() {
    // Initialize tracing for request/response logging
    tracing_subscriber::fmt::init();

    // Read configuration from environment
    let api_key = std::env::var("OKX_API_KEY").expect("OKX_API_KEY is required");
    let secret_key = std::env::var("OKX_SECRET_KEY").expect("OKX_SECRET_KEY is required");
    let passphrase = std::env::var("OKX_PASSPHRASE").expect("OKX_PASSPHRASE is required");
    let pay_to = std::env::var("PAY_TO_ADDRESS").expect("PAY_TO_ADDRESS is required");
    // 1. Configure OKX Facilitator
    // Default URL: https://web3.okx.com (override with FACILITATOR_URL env var)
    let facilitator_client = match std::env::var("FACILITATOR_URL") {
        Ok(url) => OkxHttpFacilitatorClient::with_url(&url, &api_key, &secret_key, &passphrase),
        Err(_) => OkxHttpFacilitatorClient::new(&api_key, &secret_key, &passphrase),
    }
    .expect("Failed to create facilitator client");

    // 2. Create Server and register schemes (mirrors TS: new x402ResourceServer(fc).register(n, s))
    // Register a custom MoneyParser so "$0.003" maps to USDG on X Layer
    let aggr_deferred =
        AggrDeferredEvmScheme::new().register_money_parser(Box::new(|amount, network| {
            if network == "eip155:196" {
                Some(AssetAmount {
                    asset: "0x4ae46a509f6b1d9056937ba4500cb143933d2dc8".into(),
                    amount: format!("{:.0}", amount * 1e6), // USDG: 6 decimals
                    extra: Some(HashMap::from([
                        ("name".into(), json!("USDG")),
                        ("version".into(), json!("1")),
                    ])),
                })
            } else {
                None // Other networks use default stablecoin
            }
        }));

    let mut server = X402ResourceServer::new(facilitator_client)
        .register("eip155:196", ExactEvmScheme::new())
        .register("eip155:196", aggr_deferred);

    // MUST initialize before use (fetches facilitator's supported schemes)
    // Mirrors TS: await server.initialize()
    server
        .initialize()
        .await
        .expect("Failed to initialize: check facilitator connectivity");

    // 3. Route-level payment config (mirrors TS: paymentMiddleware({ "GET /weather": {...} }, server))
    let routes = HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![
                AcceptConfig {
                    scheme: "exact".into(),
                    price: "$0.002".into(),
                    network: "eip155:196".into(),
                    pay_to: pay_to.clone(),
                    max_timeout_seconds: None,
                    extra: None,
                },
                AcceptConfig {
                    scheme: "aggr_deferred".into(),
                    price: "$0.00145".into(), // MoneyParser converts to USDG automatically
                    network: "eip155:196".into(),
                    pay_to: pay_to.clone(),
                    max_timeout_seconds: None,
                    extra: None,
                },
            ],
            description: "Get current weather data for any location".into(),
            mime_type: "application/json".into(),
            sync_settle: Some(true),
        },
    )]);

    // 4. Axum router + payment middleware (mirrors TS: app.use(paymentMiddleware(...)))
    let app = Router::new()
        .route("/weather", get(weather_handler))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4021").await.unwrap();
    println!("Server listening at http://localhost:4021");
    println!("Try: curl http://localhost:4021/weather");
    axum::serve(listener, app).await.unwrap();
}

async fn weather_handler() -> Json<Value> {
    Json(json!({
        "report": {
            "weather": "sunny",
            "temperature": 70
        }
    }))
}
