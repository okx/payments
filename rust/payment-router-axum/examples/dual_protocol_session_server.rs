//! Dual-protocol session server — `EvmMpp` with **both** charge and
//! session intents, plus x402 exact, on the same endpoint set.
//!
//! Companion to `dual_protocol_server.rs` (which only demos charge).
//! This example proves that the same `MppAdapter` correctly dispatches
//! per-route between charge and session intents through the
//! `MppRouteConfig.intent` field.
//!
//! Routes:
//! - `GET /generateImg` — MPP **charge** intent + x402 exact
//! - `GET /apiBatch`    — MPP **session** intent + x402 exact
//!
//! Session intent requires a merchant signer (EIP-712
//! `SettleAuthorization` / `CloseAuthorization`); the signer's address
//! MUST equal `MPP_RECIPIENT`.
//!
//! # Security caveat
//!
//! For brevity this example holds `MPP_MERCHANT_PRIVATE_KEY` as a plain
//! `String` inside `Env` for the process lifetime. Production code
//! should wrap it in [`secrecy::SecretString`] (zeroize-on-drop) and
//! pass it to the signer via `ExposeSecret` only at construction.
//!
//! # Running
//!
//! ```bash
//! export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
//! export MPP_SECRET_KEY=... MPP_REALM=... MPP_CURRENCY=0x... MPP_RECIPIENT=0x...
//! export MPP_MERCHANT_PRIVATE_KEY=0x<32-byte hex>   # address must == MPP_RECIPIENT
//! export X402_API_KEY=... X402_SECRET_KEY=... X402_PASSPHRASE=... X402_PAY_TO=0x...
//! cargo run --example dual_protocol_session_server
//! ```
//!
//! Quick checks (curl in a second terminal):
//!
//! ```bash
//! # Charge route — challenge has intent="charge"
//! curl -i http://localhost:4001/generateImg
//!
//! # Session route — challenge has intent="session", request contains
//! # unitType / suggestedDeposit
//! curl -i http://localhost:4001/apiBatch
//! ```

use std::sync::Arc;

use alloy_signer_local::PrivateKeySigner;
use axum::{routing::get, Json, Router};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{EvmCharge, EvmConfig, EvmMpp, EvmSessionMethod, OkxSaApiClient};
use payment_router_axum::{
    adapters::{MppAdapter, MppRouteConfig, X402Adapter, X402RouteConfig},
    PaymentRouterConfig, PaymentRouterLayer, ProtocolAdapter, UnifiedRouteConfig,
};
use serde_json::{json, Value};
use x402_axum::AcceptConfig;
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::ExactEvmScheme;

const NETWORK: &str = "eip155:196"; // X Layer mainnet

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let env = Env::load()?;

    // —— MPP setup ——
    // Both charge + session enabled on the same EvmMpp facade. The
    // adapter dispatches per-route based on MppRouteConfig.intent.
    let sa: Arc<dyn SaApiClient> = env.sa_client();
    // Don't include the parser error: alloy's hex error includes the
    // offending character/index, which on a malformed key could leak a
    // byte of MPP_MERCHANT_PRIVATE_KEY into logs.
    let signer: PrivateKeySigner = env
        .mpp_merchant_private_key
        .parse()
        .map_err(|_| "invalid MPP_MERCHANT_PRIVATE_KEY: expected 32-byte hex")?;

    let mpp = Arc::new(
        EvmMpp::builder(EvmConfig {
            chain_id: 196,
            recipient: env.mpp_recipient.clone(),
            secret_key: env.mpp_secret_key.clone(),
            realm: env.mpp_realm.clone(),
        })
        .with_charge(EvmCharge::new(sa.clone()).with_fee_payer(true))
        .with_session(EvmSessionMethod::new(sa).with_signer(signer))
        .build()?,
    );

    // —— x402 setup ——
    let facilitator = OkxHttpFacilitatorClient::new(
        &env.x402_api_key,
        &env.x402_secret_key,
        &env.x402_passphrase,
    )?;
    let mut x402_server =
        X402ResourceServer::new(facilitator).register(NETWORK, ExactEvmScheme::new());
    x402_server.initialize().await?;

    // —— Routes ——
    let charge_route = build_route(
        "AI Image Generation Service",
        MppRouteConfig {
            intent: "charge".into(),
            amount: "10".into(),
            currency: env.mpp_currency.clone(),
            description: Some("AI Image Generation Service".into()),
            external_id: None,
            unit_type: None,
            suggested_deposit: None,
        },
        env.x402_pay_to.clone(),
    );

    let session_route = build_route(
        "Batch API session",
        MppRouteConfig {
            intent: "session".into(),
            amount: "100".into(),
            currency: env.mpp_currency.clone(),
            description: Some("Batch API session".into()),
            external_id: None,
            unit_type: Some("request".into()),
            suggested_deposit: Some("10000".into()),
        },
        env.x402_pay_to.clone(),
    );

    let mpp_adapter: Arc<dyn ProtocolAdapter> = Arc::new(MppAdapter::new(mpp));
    let x402_adapter: Arc<dyn ProtocolAdapter> = Arc::new(X402Adapter::new(x402_server).build());

    let layer = PaymentRouterLayer::new(PaymentRouterConfig {
        routes: vec![
            ("GET /generateImg".into(), charge_route),
            ("GET /apiBatch".into(), session_route),
        ],
        protocols: vec![mpp_adapter, x402_adapter],
        on_error: Some(Arc::new(|err, ctx| {
            eprintln!("[{} {}] err: {err}", ctx.protocol, ctx.phase.as_str())
        })),
    })?;

    let app = Router::new()
        .route("/generateImg", get(generate_img))
        .route("/apiBatch", get(batch_handler))
        .layer(layer);

    println!("Listening on http://localhost:4001");
    println!("  GET /generateImg  — MPP charge  (10 base units) OR x402 exact ($0.00001)");
    println!("  GET /apiBatch     — MPP session (100 per unit)  OR x402 exact ($0.00001)");
    println!("\nQuick check (no Authorization → 402 dual challenge):");
    println!("  curl -i http://localhost:4001/generateImg  # intent=\"charge\"");
    println!("  curl -i http://localhost:4001/apiBatch     # intent=\"session\"");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4001").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_route(
    description: &str,
    mpp_cfg: MppRouteConfig,
    x402_pay_to: String,
) -> UnifiedRouteConfig {
    UnifiedRouteConfig::builder()
        .description(description)
        .adapter("mpp", mpp_cfg)
        .adapter(
            "x402",
            X402RouteConfig {
                accepts: vec![AcceptConfig {
                    scheme: "exact".into(),
                    price: "$0.00001".into(),
                    network: NETWORK.into(),
                    pay_to: x402_pay_to,
                    max_timeout_seconds: None,
                    extra: None,
                }],
                description: description.into(),
                mime_type: "application/json".into(),
                sync_settle: None,
                resource: None,
            },
        )
        .build()
}

async fn generate_img() -> Json<Value> {
    Json(json!({
        "imageUrl": "https://placehold.co/512x512/png?text=AI+Generated",
        "prompt": "a sunset over mountains",
    }))
}

async fn batch_handler() -> Json<Value> {
    Json(json!({
        "result": "batch operation completed",
        "items": ["a", "b", "c"],
    }))
}

// ---------------------------------------------------------------------------
// Env loading
// ---------------------------------------------------------------------------

struct Env {
    mpp_currency: String,
    mpp_recipient: String,
    mpp_realm: String,
    mpp_secret_key: String,
    mpp_sa_url: String,
    mpp_sa_key: String,
    mpp_sa_secret: String,
    mpp_sa_passphrase: String,
    mpp_merchant_private_key: String,
    x402_api_key: String,
    x402_secret_key: String,
    x402_passphrase: String,
    x402_pay_to: String,
}

impl Env {
    fn load() -> Result<Self, Box<dyn std::error::Error>> {
        fn req(name: &str) -> Result<String, Box<dyn std::error::Error>> {
            std::env::var(name).map_err(|_| format!("missing required env var: {name}").into())
        }
        Ok(Env {
            mpp_sa_url: req("MPP_SA_URL")?,
            mpp_sa_key: req("MPP_SA_KEY")?,
            mpp_sa_secret: req("MPP_SA_SECRET")?,
            mpp_sa_passphrase: req("MPP_SA_PASSPHRASE")?,
            mpp_secret_key: req("MPP_SECRET_KEY")?,
            mpp_realm: req("MPP_REALM")?,
            mpp_currency: req("MPP_CURRENCY")?,
            mpp_recipient: req("MPP_RECIPIENT")?,
            mpp_merchant_private_key: req("MPP_MERCHANT_PRIVATE_KEY")?,
            x402_api_key: req("X402_API_KEY")?,
            x402_secret_key: req("X402_SECRET_KEY")?,
            x402_passphrase: req("X402_PASSPHRASE")?,
            x402_pay_to: req("X402_PAY_TO")?,
        })
    }

    fn sa_client(&self) -> Arc<dyn SaApiClient> {
        Arc::new(OkxSaApiClient::with_base_url(
            self.mpp_sa_url.clone(),
            self.mpp_sa_key.clone(),
            self.mpp_sa_secret.clone(),
            self.mpp_sa_passphrase.clone(),
        ))
    }
}
