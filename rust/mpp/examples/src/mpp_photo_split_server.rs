//! MPP EVM Photo Server with **payment splits**.
//!
//! Same flow as `mpp_photo_server`, but `EvmChargeChallengerConfig` carries
//! `splits: Some(vec![...])`, so the issued challenge embeds
//! `ChargeMethodDetails.splits` and the client signs an independent
//! `Eip3009Authorization` per split.
//!
//! Bypasses the `MppCharge<C>` extractor and calls
//! `challenger.verify_payment()` manually so verification errors propagate
//! into the 402 response instead of being swallowed.
//!
//! # Split rules (per spec)
//!
//! - `sum(splits[].amount) < request.amount` — primary recipient must keep
//!   a non-zero remainder.
//! - Each `ChargeSplit.amount` is base-units integer string (same convention
//!   as `ChargeConfig::amount()`).
//! - Each `ChargeSplit.recipient` is a 40-hex EIP-55 address.
//!
//! Default pricing: total 100 base units (0.0001 pathUSD) → partner-a 30,
//! partner-b 20, primary keeps 50.
//!
//! # Running
//!
//! ```bash
//! export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
//! export MPP_SECRET_KEY=... MPP_REALM=split.test
//! export MPP_CURRENCY=0x... MPP_RECIPIENT=0x...
//! export MPP_SPLIT_A_RECIPIENT=0x... MPP_SPLIT_B_RECIPIENT=0x...
//! cargo run --example mpp_photo_split_server
//! ```

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger, PaymentRequired, WithReceipt};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{
    ChargeSplit, EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, OkxSaApiClient,
};
use serde_json::{json, Value};

const PHOTO_AMOUNT: &str = "100";
const PHOTO_DESCRIPTION: &str = "One photo (split across partners)";

/// Manual /photo handler — skips `MppCharge<C>` extractor so the seller can
/// surface verification errors verbatim to the client (extractor swallows
/// them).
async fn photo(
    State(challenger): State<Arc<dyn ChargeChallenger>>,
    headers: HeaderMap,
) -> Response {
    let auth_raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_raw {
        None => build_challenge_response(challenger.as_ref()),
        Some(header_val) => {
            let trimmed = header_val.trim();
            if !trimmed.to_ascii_lowercase().starts_with("payment ") {
                return build_challenge_response(challenger.as_ref());
            }
            // Pass the full header (including `Payment ` scheme) — upstream
            // strips it itself; double-stripping returns "Expected 'Payment'".
            match challenger.verify_payment(trimmed).await {
                Ok(receipt) => WithReceipt {
                    receipt,
                    body: Json(json!({ "url": "https://picsum.photos/id/42/1024/1024.jpg" })),
                }
                .into_response(),
                Err(_) => build_challenge_response(challenger.as_ref()),
            }
        }
    }
}

fn build_challenge_response(challenger: &dyn ChargeChallenger) -> Response {
    let options = ChallengeOptions {
        description: Some(PHOTO_DESCRIPTION),
    };
    match challenger.challenge(PHOTO_AMOUNT, options) {
        Ok(ch) => PaymentRequired(ch).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("challenge: {e}")).into_response(),
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let env = load_env();
    let sa_client = env.build_sa_client();

    // Splits are configured once at the challenger level — every issued
    // challenge embeds them in `ChargeMethodDetails.splits`.
    let splits = vec![
        ChargeSplit {
            amount: "30".into(),
            recipient: env.split_a_recipient.clone(),
            memo: Some("partner-a".into()),
        },
        ChargeSplit {
            amount: "20".into(),
            recipient: env.split_b_recipient.clone(),
            memo: Some("partner-b".into()),
        },
    ];

    let challenger: Arc<dyn ChargeChallenger> = Arc::new(EvmChargeChallenger::new(
        EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(sa_client),
            currency: env.currency.clone(),
            recipient: env.recipient.clone(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: env.realm.clone(),
            secret_key: env.secret_key,
            splits: Some(splits),
        },
    ));

    println!("Realm:        {}", env.realm);
    println!("Currency:     {}", env.currency);
    println!("Primary:      {} (keeps 50 base units)", env.recipient);
    println!("Split A:      {} → 30 base units", env.split_a_recipient);
    println!("Split B:      {} → 20 base units", env.split_b_recipient);
    println!("Total amount: 0.0001 pathUSD (100 base units, 6 decimals)");

    let app = Router::new()
        .route("/health", get(health))
        .route("/photo", get(photo))
        .with_state(challenger);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4024")
        .await
        .expect("bind failed");
    println!("\nListening on http://localhost:4024");
    println!("  GET /health  — free");
    println!("  GET /photo   — 0.0001 pathUSD (split 50/30/20)");
    println!("\nTest: curl -D- http://localhost:4024/photo\n");

    axum::serve(listener, app).await.expect("server error");
}

struct Env {
    sa_url: String,
    sa_key: String,
    sa_secret: String,
    sa_passphrase: String,
    secret_key: String,
    realm: String,
    currency: String,
    recipient: String,
    split_a_recipient: String,
    split_b_recipient: String,
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
                MPP_SECRET_KEY MPP_REALM MPP_CURRENCY MPP_RECIPIENT \
                MPP_SPLIT_A_RECIPIENT MPP_SPLIT_B_RECIPIENT"
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
        split_a_recipient: required("MPP_SPLIT_A_RECIPIENT"),
        split_b_recipient: required("MPP_SPLIT_B_RECIPIENT"),
    }
}
