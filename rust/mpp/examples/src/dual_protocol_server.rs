//! End-to-end dual-protocol (MPP + x402) server example — spec §1 Adapter
//! pattern.
//!
//! Same endpoint `/photo` serves either protocol:
//!
//! - `Authorization: Payment <b64>`  → MPP flow (MppAdapter → EvmChargeChallenger)
//! - `X-Payment: <b64>`               → x402 flow (X402Adapter → native PaymentMiddleware)
//! - Neither header                   → 402 with multi-row WWW-Authenticate + PAYMENT-REQUIRED
//!
//! Business handler is registered **once** and is protocol-agnostic. Headers
//! (Payment-Receipt / PAYMENT-RESPONSE) are injected by each adapter's
//! wrapped service. No MPP/x402 code is duplicated or patched — both use
//! their native middleware.
//!
//! # Running
//!
//! ```bash
//! # MPP-only mock (fastest local demo; x402 adapter skipped unless env set)
//! MPP_MOCK=1 cargo run --example dual_protocol_server
//!
//! # Full dual-protocol with real x402 facilitator
//! MPP_MOCK=1 \
//!   X402_API_KEY=... X402_SECRET_KEY=... X402_PASSPHRASE=... \
//!   X402_PAY_TO=0x4b22fdbc399bd422b6fefcbce95f76642ea29df1 \
//!   cargo run --example dual_protocol_server
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use axum::{routing::get, Json, Router};
use mpp::server::axum::ChargeChallenger;
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{
    EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, MockSaApiClient,
    OkxSaApiClient,
};
use payment_router_axum::{
    adapters::{MppAdapter, X402Adapter},
    PaymentRouterConfig, PaymentRouterLayer, ProtocolAdapter, UnifiedRouteConfig,
};
use serde_json::{json, Value};
use x402_axum::{AcceptConfig, RoutePaymentConfig, RoutesConfig};
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::server::X402ResourceServer;
use x402_evm::ExactEvmScheme;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    println!("=== Dual-Protocol Payment Server (MPP + x402) ===\n");

    let mpp_env = load_mpp_env();
    let x402_env = load_x402_env();

    // ------------- MPP side -------------
    let sa_client = mpp_env.build_sa_client();
    let challenger: Arc<dyn ChargeChallenger> = Arc::new(EvmChargeChallenger::new(
        EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(sa_client),
            currency: mpp_env.currency.clone(),
            recipient: mpp_env.recipient.clone(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: mpp_env.realm.clone(),
            secret_key: mpp_env.secret_key.clone(),
            splits: None,
        },
    ));
    let mpp_adapter: Arc<dyn ProtocolAdapter> = Arc::new(MppAdapter::new(challenger));

    // ------------- x402 side (optional) -------------
    //
    // x402 requires a real facilitator to call `initialize()`. In local
    // mock-only mode we register a dummy adapter slot with a fake facilitator
    // URL, which will generate 402 challenges but fail at verify time —
    // acceptable for demonstrating the 402 multi-row path without a real
    // x402 client.
    let mut protocols: Vec<Arc<dyn ProtocolAdapter>> = vec![mpp_adapter];
    let mut x402_registered = false;

    if let Some(ref x402) = x402_env {
        let facilitator = match std::env::var("X402_FACILITATOR_URL") {
            Ok(url) => OkxHttpFacilitatorClient::with_url(
                &url,
                &x402.api_key,
                &x402.secret_key,
                &x402.passphrase,
            ),
            Err(_) => OkxHttpFacilitatorClient::new(
                &x402.api_key,
                &x402.secret_key,
                &x402.passphrase,
            ),
        }?;

        let mut server = X402ResourceServer::new(facilitator)
            .register("eip155:196", ExactEvmScheme::new());
        server.initialize().await?;

        let routes = x402_routes_config(&x402.pay_to);
        let x402_adapter: Arc<dyn ProtocolAdapter> = Arc::new(
            X402Adapter::builder(routes, server).build(),
        );
        protocols.push(x402_adapter);
        x402_registered = true;
    }

    // ------------- PaymentRouter setup -------------
    let mpp_cfg = json!({
        "amount": "100",
        "description": "One photo",
    });
    let x402_cfg = json!({
        "scheme": "exact",
        "price": "$0.01",
        "network": "eip155:196",
        "payTo": x402_env.as_ref().map(|e| e.pay_to.as_str()).unwrap_or("0x0000000000000000000000000000000000000000"),
    });
    let mut adapter_configs = HashMap::new();
    adapter_configs.insert("mpp".into(), mpp_cfg);
    if x402_registered {
        adapter_configs.insert("x402".into(), x402_cfg);
    }

    let layer = PaymentRouterLayer::new(PaymentRouterConfig {
        routes: vec![(
            "GET /photo".into(),
            UnifiedRouteConfig {
                description: Some("dual-protocol photo".into()),
                adapter_configs,
            },
        )],
        protocols,
        on_error: Some(Arc::new(|err, ctx| {
            eprintln!(
                "[{} {}] err: {err}",
                ctx.protocol,
                ctx.phase.as_str(),
            );
        })),
    })?;

    // ------------- axum Router -------------
    let app = Router::new()
        .route("/health", get(health))
        .route("/photo", get(photo_handler))
        .with_state(challenger_state()?)  // harmless fallback state
        .layer(layer);

    println!("Listening on http://localhost:4023");
    println!("  GET /health              free (no payment required)");
    println!("  GET /photo               paid: MPP 100 base units OR x402 $0.01");
    println!();
    println!("Try:");
    println!("  curl -i http://localhost:4023/photo                      # 402 dual challenge");
    println!("  curl -i -H 'Authorization: Payment <b64>' http://localhost:4023/photo");
    println!("  curl -i -H 'X-Payment: <b64>' http://localhost:4023/photo");
    if !x402_registered {
        println!("\n⚠ x402 adapter NOT registered (X402_API_KEY etc. missing)");
        println!("  Set env vars to enable both protocols. 402 will show MPP row only.");
    }

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4023").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// Business handler — protocol-agnostic. Runs only after payment passes.
async fn photo_handler() -> Json<Value> {
    Json(json!({ "url": "https://picsum.photos/id/42/1024/1024.jpg" }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// Dummy state satisfier; MPP extractor path isn't used here (we go through the
// adapter, not `MppCharge<C>`). Kept only so `with_state` type-checks.
fn challenger_state() -> Result<Arc<dyn ChargeChallenger>, Box<dyn std::error::Error>> {
    Ok(Arc::new(EvmChargeChallenger::new(
        EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(Arc::new(MockSaApiClient::new()) as Arc<dyn SaApiClient>),
            currency: "0x0000000000000000000000000000000000000000".into(),
            recipient: "0x0000000000000000000000000000000000000000".into(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: "state-fallback".into(),
            secret_key: "unused".into(),
            splits: None,
        },
    )))
}

fn x402_routes_config(pay_to: &str) -> RoutesConfig {
    HashMap::from([(
        "GET /photo".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "exact".into(),
                price: "$0.01".into(),
                network: "eip155:196".into(),
                pay_to: pay_to.into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "dual-protocol photo".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        },
    )])
}

// ---------------------------------------------------------------------------
// Env handling
// ---------------------------------------------------------------------------

struct MppEnv {
    currency: String,
    recipient: String,
    realm: String,
    secret_key: String,
    mock: bool,
    sa_url: String,
    sa_key: String,
    sa_secret: String,
    sa_passphrase: String,
}

impl MppEnv {
    fn build_sa_client(&self) -> Arc<dyn SaApiClient> {
        if self.mock {
            Arc::new(MockSaApiClient::new())
        } else {
            Arc::new(OkxSaApiClient::with_base_url(
                self.sa_url.clone(),
                self.sa_key.clone(),
                self.sa_secret.clone(),
                self.sa_passphrase.clone(),
            ))
        }
    }
}

fn load_mpp_env() -> MppEnv {
    if std::env::var("MPP_MOCK").ok().as_deref() == Some("1") {
        println!("[MPP] MPP_MOCK=1 → MockSaApiClient (no real SA calls)");
        return MppEnv {
            sa_url: "http://mock.local".into(),
            sa_key: "mock".into(),
            sa_secret: "mock".into(),
            sa_passphrase: "mock".into(),
            secret_key: "mock-hmac-secret".into(),
            realm: "mock.local".into(),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            mock: true,
        };
    }
    let req = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("missing env var: {k}"));
    MppEnv {
        sa_url: req("MPP_SA_URL"),
        sa_key: req("MPP_SA_KEY"),
        sa_secret: req("MPP_SA_SECRET"),
        sa_passphrase: req("MPP_SA_PASSPHRASE"),
        secret_key: req("MPP_SECRET_KEY"),
        realm: req("MPP_REALM"),
        currency: req("MPP_CURRENCY"),
        recipient: req("MPP_RECIPIENT"),
        mock: false,
    }
}

struct X402Env {
    api_key: String,
    secret_key: String,
    passphrase: String,
    pay_to: String,
}

fn load_x402_env() -> Option<X402Env> {
    let api_key = std::env::var("X402_API_KEY").ok()?;
    let secret_key = std::env::var("X402_SECRET_KEY").ok()?;
    let passphrase = std::env::var("X402_PASSPHRASE").ok()?;
    let pay_to = std::env::var("X402_PAY_TO").ok()?;
    Some(X402Env {
        api_key,
        secret_key,
        passphrase,
        pay_to,
    })
}
