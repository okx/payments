//! Upto scheme demo. The handler sets a `settlement-overrides` header to
//! charge less than the cap; supported override formats:
//! `{"amount": "1234000"}` | `{"amount": "50%"}` | `{"amount": "$0.123"}` |
//! `{"amount": "0"}` (short-circuits, no chain tx).
//!
//! Run: `cargo run --example upto_server`

use std::collections::HashMap;

use axum::{response::IntoResponse, response::Response, routing::get, Json, Router};
use serde_json::json;

use x402_axum::{
    payment_middleware, AcceptConfig, RoutePaymentConfig, SETTLEMENT_OVERRIDES_HEADER,
};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::UptoEvmScheme;

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
        .register("eip155:196", UptoEvmScheme::new());

    server
        .initialize()
        .await
        .expect("Failed to initialize: check facilitator connectivity");

    // `price` is the cap; `extra.assetTransferMethod` and
    // `extra.facilitatorAddress` are auto-injected by `UptoEvmScheme`.
    let routes = HashMap::from([(
        "GET /usage".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "upto".into(),
                price: "$0.10".into(),
                network: "eip155:196".into(),
                pay_to: pay_to.clone(),
                max_timeout_seconds: Some(300),
                extra: None,
            }],
            description: "Pay-by-actual-usage demo (upto)".into(),
            mime_type: "application/json".into(),
            sync_settle: Some(true),
            resource: None,
        },
    )]);

    let app = Router::new()
        .route("/usage", get(usage_handler))
        .layer(payment_middleware(routes, server));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4022").await.unwrap();
    println!("Server listening at http://localhost:4022 (upto mode, cap $0.10)");
    println!("Try: curl http://localhost:4022/usage");
    axum::serve(listener, app).await.unwrap();
}

async fn usage_handler() -> Response {
    let actual_amount = "$0.034";

    let body = Json(json!({
        "report": { "tokens_used": 1342, "model": "demo" },
        "billed": actual_amount,
    }));

    let mut resp = (axum::http::StatusCode::OK, body).into_response();

    // Middleware reads this header to override the settle amount, then
    // strips it from the outgoing response.
    resp.headers_mut().insert(
        SETTLEMENT_OVERRIDES_HEADER,
        format!(r#"{{"amount":"{}"}}"#, actual_amount)
            .parse()
            .unwrap(),
    );

    resp
}

