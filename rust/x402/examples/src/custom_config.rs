//! Advanced Axum server with multiple payment-protected routes.
//!
//! Demonstrates:
//! - Multiple routes with different prices
//! - Both exact and deferred schemes
//! - Custom facilitator URL (test environment)
//!
//! Run: `cargo run --example custom_config`

use std::collections::HashMap;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use x402_axum::{payment_middleware, AcceptConfig, RoutePaymentConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::{AggrDeferredEvmScheme, ExactEvmScheme};

#[tokio::main]
async fn main() {
    let api_key = std::env::var("OKX_API_KEY").expect("OKX_API_KEY is required");
    let secret_key = std::env::var("OKX_SECRET_KEY").expect("OKX_SECRET_KEY is required");
    let passphrase = std::env::var("OKX_PASSPHRASE").expect("OKX_PASSPHRASE is required");
    let pay_to = std::env::var("PAY_TO_ADDRESS").expect("PAY_TO_ADDRESS is required");

    // Default URL: https://web3.okx.com (override with FACILITATOR_URL env var)
    let facilitator_client = match std::env::var("FACILITATOR_URL") {
        Ok(url) => OkxHttpFacilitatorClient::with_url(&url, &api_key, &secret_key, &passphrase),
        Err(_) => OkxHttpFacilitatorClient::new(&api_key, &secret_key, &passphrase),
    }
    .expect("Failed to create facilitator client");

    let mut server = X402ResourceServer::new(facilitator_client)
        .register("eip155:196", ExactEvmScheme::new())
        .register("eip155:196", AggrDeferredEvmScheme::new());

    // MUST initialize before use (fetches facilitator's supported schemes)
    server
        .initialize()
        .await
        .expect("Failed to initialize: check facilitator connectivity");

    // Multiple routes with different prices
    let routes = HashMap::from([
        (
            "GET /api/weather".to_string(),
            RoutePaymentConfig {
                accepts: vec![
                    AcceptConfig {
                        scheme: "exact".into(),
                        price: "$0.001".into(),
                        network: "eip155:196".into(),
                        pay_to: pay_to.clone(),
                        max_timeout_seconds: None,
                        extra: None,
                    },
                    AcceptConfig {
                        scheme: "aggr_deferred".into(),
                        price: "$0.001".into(),
                        network: "eip155:196".into(),
                        pay_to: pay_to.clone(),
                        max_timeout_seconds: None,
                        extra: None,
                    },
                ],
                description: "Get current weather data".into(),
                mime_type: "application/json".into(),
                sync_settle: None,
            },
        ),
        (
            "GET /api/premium".to_string(),
            RoutePaymentConfig {
                accepts: vec![AcceptConfig {
                    scheme: "exact".into(),
                    price: "$0.01".into(),
                    network: "eip155:196".into(),
                    pay_to: pay_to.clone(),
                    max_timeout_seconds: None,
                    extra: None,
                }],
                description: "Premium analytics data".into(),
                mime_type: "application/json".into(),
                sync_settle: None,
            },
        ),
    ]);

    let app = Router::new()
        .route("/api/weather", get(weather_handler))
        .route("/api/premium", get(premium_handler))
        .route("/health", get(health_handler)) // Free endpoint, no payment
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4021").await.unwrap();
    println!("Server listening at http://localhost:4021");
    println!("Free:    curl http://localhost:4021/health");
    println!("Paid:    curl http://localhost:4021/api/weather   ($0.001)");
    println!("Premium: curl http://localhost:4021/api/premium   ($0.01)");
    axum::serve(listener, app).await.unwrap();
}

async fn weather_handler() -> Json<Value> {
    Json(json!({
        "report": { "weather": "sunny", "temperature": 70 }
    }))
}

async fn premium_handler() -> Json<Value> {
    Json(json!({
        "analytics": {
            "daily_active_users": 15420,
            "revenue": "$12,340",
            "conversion_rate": "3.2%"
        }
    }))
}

async fn health_handler() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
