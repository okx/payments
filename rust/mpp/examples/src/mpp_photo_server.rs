//! MPP EVM Payment-Gated Photo Server — ergonomic-extractor edition.
//!
//! Uses upstream `MppCharge<C>` extractor + `WithReceipt<T>` response
//! wrapper + our `EvmChargeChallenger` (`impl ChargeChallenger`), which
//! eliminates the boilerplate of hand-written parse / verify /
//! format_www_authenticate. The whole server fits on one screen.
//!
//! Flow (HTTP behavior is identical to the older hand-rolled example):
//! 1. `GET /photo` without Authorization → 402 +
//!    `WWW-Authenticate: Payment ...`
//! 2. Client signs an EIP-3009 voucher and retries with
//!    `Authorization: Payment <base64url>`
//! 3. Server verifies + deducts via SA API → 200 + `Payment-Receipt`
//!    header + photo URL
//!
//! # Running (real SA API)
//!
//! ```bash
//! export MPP_SA_URL=...
//! export MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
//! export MPP_SECRET_KEY=photo-demo-secret MPP_REALM=photo.test
//! export MPP_CURRENCY=0x74b7F16337b8972027F6196A17a631aC6dE26d22
//! export MPP_RECIPIENT=0x4b22fdbc399bd422b6fefcbce95f76642ea29df1
//! cargo run --example mpp_photo_server
//! ```

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use mpp::server::axum::{ChargeChallenger, ChargeConfig, MppCharge, WithReceipt};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, OkxSaApiClient};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Per-route price — `MppCharge<C>` reads `C::amount()` on every request.
// ---------------------------------------------------------------------------

/// 100 base units of pathUSD (6 decimals) = 0.0001 pathUSD.
///
/// `amount()` MUST be a base-units integer string — the MPP protocol
/// requires `request.amount` to be a "base-10 integer string with no
/// sign, decimal point, or exponent". Do NOT write "0.0001" (a dollar-
/// style decimal): that's an upstream Tempo backend convenience, not
/// part of the protocol spec.
struct OnePhoto;
impl ChargeConfig for OnePhoto {
    fn amount() -> &'static str {
        "100"
    }
    fn description() -> Option<&'static str> {
        Some("One photo")
    }
}

// ---------------------------------------------------------------------------
// Business handler — `MppCharge<OnePhoto>` extracts only after the
// payment is verified. `WithReceipt` wraps the response and attaches
// the `Payment-Receipt` header automatically.
// ---------------------------------------------------------------------------

async fn photo(charge: MppCharge<OnePhoto>) -> WithReceipt<Json<Value>> {
    // Return a fixed sample URL with no outbound HTTP call, so this works
    // in offline / onchainos integration environments.
    WithReceipt {
        receipt: charge.receipt,
        body: Json(json!({ "url": "https://picsum.photos/id/42/1024/1024.jpg" })),
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// ---------------------------------------------------------------------------
// main: build the challenger -> install it as axum state -> serve.
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let env = load_env();
    let sa_client = env.build_sa_client();
    let challenger: Arc<dyn ChargeChallenger> =
        Arc::new(EvmChargeChallenger::new(EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(sa_client),
            currency: env.currency.clone(),
            recipient: env.recipient.clone(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: env.realm.clone(),
            secret_key: env.secret_key,
            splits: None,
        }));

    println!("Realm:     {}", env.realm);
    println!("Recipient: {}", env.recipient);
    println!("Currency:  {}", env.currency);
    println!("Price:     0.0001 pathUSD (100 base units, 6 decimals)");

    let app = Router::new()
        .route("/health", get(health))
        .route("/photo", get(photo))
        .with_state(challenger);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4022")
        .await
        .expect("bind failed");
    println!("\nListening on http://localhost:4022");
    println!("  GET /health  — free");
    println!("  GET /photo   — 0.0001 pathUSD");
    println!("\nTest: curl -D- http://localhost:4022/photo\n");

    axum::serve(listener, app).await.expect("server error");
}

// ---------------------------------------------------------------------------
// Env loading — all SA API credentials and merchant config are required.
// ---------------------------------------------------------------------------

struct Env {
    sa_url: String,
    sa_key: String,
    sa_secret: String,
    sa_passphrase: String,
    secret_key: String,
    realm: String,
    currency: String,
    recipient: String,
}

impl Env {
    fn build_sa_client(&self) -> Arc<dyn SaApiClient> {
        Arc::new(OkxSaApiClient::with_base_url(
            self.sa_url.clone(),
            self.sa_key.clone(),
            self.sa_secret.clone(),
            self.sa_passphrase.clone(),
        ))
    }
}

fn load_env() -> Env {
    let required = |k: &str| {
        std::env::var(k).unwrap_or_else(|_| {
            eprintln!("missing env var: {k}");
            eprintln!(
                "required: MPP_SA_URL MPP_SA_KEY MPP_SA_SECRET MPP_SA_PASSPHRASE \
                MPP_SECRET_KEY MPP_REALM MPP_CURRENCY MPP_RECIPIENT"
            );
            std::process::exit(1);
        })
    };
    Env {
        sa_url: required("MPP_SA_URL"),
        sa_key: required("MPP_SA_KEY"),
        sa_secret: required("MPP_SA_SECRET"),
        sa_passphrase: required("MPP_SA_PASSPHRASE"),
        secret_key: required("MPP_SECRET_KEY"),
        realm: required("MPP_REALM"),
        currency: required("MPP_CURRENCY"),
        recipient: required("MPP_RECIPIENT"),
    }
}
