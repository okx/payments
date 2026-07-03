//! Exact + Permit2 scheme demo. The buyer must have approved the canonical
//! Permit2 contract for the chosen ERC-20 once before any payment, or the
//! facilitator's settle reverts.
//!
//! Run: `cargo run --example exact_permit2_server`

use std::collections::HashMap;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use x402_axum::{payment_middleware, AcceptConfig, RoutePaymentConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::ExactEvmScheme;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("OKX_API_KEY").expect("OKX_API_KEY is required");
    let secret_key = std::env::var("OKX_SECRET_KEY").expect("OKX_SECRET_KEY is required");
    let passphrase = std::env::var("OKX_PASSPHRASE").expect("OKX_PASSPHRASE is required");
    let pay_to = std::env::var("PAY_TO_ADDRESS").expect("PAY_TO_ADDRESS is required");

    let facilitator_client =
        OkxHttpFacilitatorClient::new(&api_key, &secret_key, &passphrase)
            .expect("Failed to create facilitator client");

    let mut server = X402ResourceServer::new(facilitator_client)
        .register("eip155:196", ExactEvmScheme::new());

    server
        .initialize()
        .await
        .expect("Failed to initialize: check facilitator connectivity");

    // `extra.assetTransferMethod = "permit2"` tells the buyer to sign Permit2
    // instead of EIP-3009.
    let routes = HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
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
            description: "Weather data — paid via Permit2 (any ERC-20 on X Layer)".into(),
            mime_type: "application/json".into(),
            sync_settle: Some(true),
            resource: None,
            operation: None,
        },
    )]);

    let app = Router::new()
        .route("/weather", get(weather_handler))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4021").await.unwrap();
    println!("Server listening at http://localhost:4021 (Permit2 mode)");
    println!("Try: curl http://localhost:4021/weather");
    println!();
    println!("Buyer must have approved PERMIT2 (0x000000000022D473030F116dDEE9F6B43aC78BA3)");
    println!("to spend the chosen ERC-20 token before calling, or settle will revert.");
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
