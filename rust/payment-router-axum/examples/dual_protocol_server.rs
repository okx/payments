//! Dual-protocol (MPP + x402) server — payment-router-axum example.
//!
//! 1. Build `EvmMpp` (charge method only here; uncomment `with_session`
//!    to add session intent).
//! 2. Build x402 `X402ResourceServer` + `.initialize()`.
//! 3. Declare each route once on `UnifiedRouteConfig` — per-adapter
//!    `MppRouteConfig` / `X402RouteConfig` are typed structs (not JSON).
//! 4. `PaymentRouterLayer::new` walks the unified routes, calls each
//!    adapter's `prepare()` hook (x402 derives its `RoutesConfig` from
//!    the per-route `X402RouteConfig`).
//!
//! Behavior:
//! - `Authorization: Payment <b64>` → MPP charge/session flow
//! - `X-Payment: <b64>`              → x402 flow
//! - Neither header                  → 402 with multi-row WWW-Authenticate
//!
//! # Running
//!
//! ```bash
//! export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
//! export MPP_SECRET_KEY=... MPP_REALM=... MPP_CURRENCY=0x... MPP_RECIPIENT=0x...
//! export X402_API_KEY=... X402_SECRET_KEY=... X402_PASSPHRASE=... X402_PAY_TO=0x...
//! cargo run --example dual_protocol_server
//! ```

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{EvmCharge, EvmConfig, EvmMpp, OkxSaApiClient};
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

    // —— MPP setup —— service-level config + charge method.
    // Add `.with_session(EvmSessionMethod::new(sa).with_signer(signer))`
    // to enable session intent.
    let sa: Arc<dyn SaApiClient> = env.sa_client();
    let mpp = Arc::new(
        EvmMpp::builder(EvmConfig {
            chain_id: 196,
            recipient: env.mpp_recipient.clone(),
            secret_key: env.mpp_secret_key.clone(),
            realm: env.mpp_realm.clone(),
        })
        .with_charge(EvmCharge::new(sa).with_fee_payer(true))
        .build()?,
    );

    // —— x402 setup —— facilitator + scheme. `initialize()` must run
    // before the adapter sees its first request (we do it here so the
    // example fails fast on credential issues).
    let facilitator = OkxHttpFacilitatorClient::new(
        &env.x402_api_key,
        &env.x402_secret_key,
        &env.x402_passphrase,
    )?;
    let mut x402_server =
        X402ResourceServer::new(facilitator).register(NETWORK, ExactEvmScheme::new());
    x402_server.initialize().await?;

    // —— Router setup —— Routes are declared once. `PaymentRouterLayer`
    // distributes the per-adapter typed configs to each adapter's
    // `prepare()` hook (x402 builds its internal RoutesConfig from
    // `X402RouteConfig` here; MPP reads `MppRouteConfig` per-request).
    let route = UnifiedRouteConfig::builder()
        .description("AI Image Generation Service")
        .adapter(
            "mpp",
            MppRouteConfig {
                intent: "charge".into(),
                amount: "10".into(),
                currency: env.mpp_currency.clone(),
                description: Some("AI Image Generation Service".into()),
                external_id: None,
                unit_type: None,
                suggested_deposit: None,
            },
        )
        .adapter(
            "x402",
            X402RouteConfig {
                accepts: vec![AcceptConfig {
                    scheme: "exact".into(),
                    price: "$0.00001".into(),
                    network: NETWORK.into(),
                    pay_to: env.x402_pay_to.clone(),
                    max_timeout_seconds: None,
                    extra: None,
                }],
                description: "AI Image Generation Service".into(),
                mime_type: "application/json".into(),
                sync_settle: None,
                resource: None,
            },
        )
        .build();

    let mpp_adapter: Arc<dyn ProtocolAdapter> = Arc::new(MppAdapter::new(mpp));
    let x402_adapter: Arc<dyn ProtocolAdapter> = Arc::new(X402Adapter::new(x402_server).build());

    let layer = PaymentRouterLayer::new(PaymentRouterConfig {
        routes: vec![("GET /generateImg".into(), route)],
        protocols: vec![mpp_adapter, x402_adapter],
        on_error: Some(Arc::new(|err, ctx| {
            eprintln!("[{} {}] err: {err}", ctx.protocol, ctx.phase.as_str())
        })),
    })?;

    // —— axum wiring —— Protocol-agnostic handler. Runs only after
    // one of the adapters has verified payment.
    let app = Router::new()
        .route("/generateImg", get(generate_img))
        .layer(layer);

    println!("Listening on http://localhost:4000");
    println!("  GET /generateImg  — MPP charge (10 base units) OR x402 exact ($0.00001)");
    println!("\nTry:");
    println!("  curl -i http://localhost:4000/generateImg                            # 402 dual challenge");
    println!("  curl -i -H 'Authorization: Payment <b64>' http://localhost:4000/generateImg");
    println!("  curl -i -H 'X-Payment: <b64>' http://localhost:4000/generateImg");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Protocol-agnostic business handler. The payment layer ensures we
/// only reach this code after a successful charge / session voucher
/// verify.
async fn generate_img() -> Json<Value> {
    Json(json!({
        "imageUrl": "https://placehold.co/512x512/png?text=AI+Generated",
        "prompt": "a sunset over mountains",
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
    x402_api_key: String,
    x402_secret_key: String,
    x402_passphrase: String,
    x402_pay_to: String,
}

impl Env {
    fn load() -> Result<Self, Box<dyn std::error::Error>> {
        fn req(name: &str) -> Result<String, Box<dyn std::error::Error>> {
            std::env::var(name)
                .map_err(|_| format!("missing required env var: {name}").into())
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
