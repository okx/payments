//! MPP EVM Session Server (non-SSE)
//!
//! Demonstrates the non-streaming session flow: each HTTP request debits a
//! unit against an already-opened channel. No `payment-need-voucher` SSE
//! events, no long-lived stream — purely request/response.
//!
//! Endpoints:
//! - `POST /session/manage` — accepts open/voucher/topUp/close credentials.
//!   Returns 200 with management body for open/topUp/close, or 200 with a
//!   business payload for voucher (one "unit" of service per request).
//! - `POST /session/settle` — seller-initiated mid-session settlement.
//! - `GET  /session/status` — channel state lookup.
//!
//! Running (MOCK mode, no real SA API needed — best for testing client/SDK
//! EIP-712 alignment with onchainos):
//!
//! ```bash
//! export MPP_MOCK=1
//! cargo run -p mpp-examples --example mpp_session_server
//! ```
//!
//! In MOCK mode, the merchant signer is a fixed demo private key whose address
//! is auto-set as the channel recipient (so payee-consistency checks pass).
//!
//! Running against real SA API:
//!
//! ```bash
//! export MPP_SA_URL=... MPP_SA_KEY=... MPP_SA_SECRET=... MPP_SA_PASSPHRASE=...
//! export MPP_SECRET_KEY=session-demo-secret MPP_REALM=session.test
//! export MPP_CURRENCY=0x74b7F16337b8972027F6196A17a631aC6dE26d22
//! export MPP_RECIPIENT=0x4b22fdbc399bd422b6fefcbce95f76642ea29df1
//! export MPP_ESCROW=0x1234567890abcdef1234567890abcdef12345678
//! # MUST match MPP_RECIPIENT — payee signer for SettleAuthorization / CloseAuthorization
//! export MPP_MERCHANT_PRIVATE_KEY=0x<32-byte hex>
//!
//! cargo run -p mpp-examples --example mpp_session_server
//! ```

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use mpp::protocol::core::{format_www_authenticate, parse_authorization};
use mpp::protocol::traits::SessionMethod;
use mpp_evm::challenge::{build_session_challenge, session_request_with};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::types::{ChannelStatus, SessionMethodDetails};
use mpp_evm::{EvmSessionMethod, MockSaApiClient, OkxSaApiClient};
use alloy_signer_local::PrivateKeySigner;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const UNIT_PRICE_BASE_UNITS: &str = "100";
const SUGGESTED_DEPOSIT: &str = "10000";
const UNIT_TYPE: &str = "request";

struct AppState {
    session_method: Arc<EvmSessionMethod>,
    secret_key: String,
    realm: String,
    currency: String,
    recipient: String,
    escrow: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    println!("=== MPP EVM Session Server (non-SSE) ===\n");

    let (sa_client, mut cfg, signer) = load_client_and_config();

    // MOCK 模式下让 recipient = signer.address(),保证 SDK 的 payee consistency check
    // (handle_open 时校验 signer.address() == challenge.request.recipient)
    let signer_addr = format!("{:#x}", signer.address());
    if std::env::var("MPP_MOCK").ok().as_deref() == Some("1") {
        cfg.recipient = signer_addr.clone();
    }

    let session_method = Arc::new(
        EvmSessionMethod::new(sa_client)
            .with_escrow(&cfg.escrow)
            .with_signer(signer),
    );

    println!("Realm:       {}", cfg.realm);
    println!("Recipient:   {}  (← payee signer address)", cfg.recipient);
    println!("Currency:    {}", cfg.currency);
    println!("Escrow:      {}", cfg.escrow);
    println!("Unit price:  {UNIT_PRICE_BASE_UNITS} base units / {UNIT_TYPE}");

    let state = Arc::new(AppState {
        session_method: session_method.clone(),
        secret_key: cfg.secret_key,
        realm: cfg.realm,
        currency: cfg.currency,
        recipient: cfg.recipient,
        escrow: cfg.escrow,
    });

    let app = Router::new()
        .route("/session/manage", post(manage))
        .route("/session/settle", post(settle))
        .route("/session/status", get(status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4023")
        .await
        .expect("bind failed");

    println!("\nListening on http://localhost:4023");
    println!("  POST /session/manage — open/voucher/topUp/close");
    println!("  POST /session/settle — seller mid-session settle");
    println!("  GET  /session/status?channelId=0x... — channel state\n");

    axum::serve(listener, app).await.expect("server error");
}

struct Config {
    sa_url: String,
    sa_key: String,
    sa_secret: String,
    sa_passphrase: String,
    secret_key: String,
    realm: String,
    currency: String,
    recipient: String,
    escrow: String,
}

/// MOCK 模式专用的固定 demo 私钥(端到端测试稳定性,不要在生产复用)。
/// 派生地址会自动覆盖 cfg.recipient,保证 payee consistency check 通过。
const MOCK_MERCHANT_PK: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// 装载 SA API client + Config + payee Signer。
///
/// - `MPP_MOCK=1` 走 `MockSaApiClient` + 固定 demo 私钥,不需要任何真实凭证,
///   方便本地 dev / 跟 onchainos cli 跑端到端 EIP-712 对齐验证。
/// - 否则走真实 `OkxSaApiClient`,所有 env var 都必填(包括
///   `MPP_MERCHANT_PRIVATE_KEY` —— 必须与 `MPP_RECIPIENT` 同地址)。
fn load_client_and_config() -> (Arc<dyn SaApiClient>, Config, PrivateKeySigner) {
    if std::env::var("MPP_MOCK").ok().as_deref() == Some("1") {
        println!("⚠ MPP_MOCK=1 — using MockSaApiClient + fixed demo merchant key");
        // mock 模式下 escrow / currency 都允许从 env 覆盖,方便用真实部署的合约
        // 地址做 EIP-712 域对齐验证(SDK + onchainos cli + 链上合约 三方对齐)。
        let escrow_override = std::env::var("MPP_ESCROW").ok();
        let currency_override = std::env::var("MPP_CURRENCY").ok();
        let cfg = Config {
            sa_url: "http://mock.local".into(),
            sa_key: "mock".into(),
            sa_secret: "mock".into(),
            sa_passphrase: "mock".into(),
            secret_key: "mock-hmac-secret".into(),
            realm: "mock.local".into(),
            currency: currency_override
                .unwrap_or_else(|| "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into()),
            recipient: "0x0000000000000000000000000000000000000000".into(), // 启动时被 signer.address() 覆盖
            escrow: escrow_override
                .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into()),
        };
        let signer: PrivateKeySigner = MOCK_MERCHANT_PK
            .parse()
            .expect("MOCK_MERCHANT_PK is hardcoded valid");
        let client: Arc<dyn SaApiClient> = Arc::new(MockSaApiClient::new());
        (client, cfg, signer)
    } else {
        let cfg = match load_config() {
            Ok(c) => c,
            Err(missing) => {
                eprintln!("missing env var: {missing}");
                eprintln!("tip: set MPP_MOCK=1 to run with mocked SA API (no creds needed)");
                eprintln!(
                    "required: MPP_SA_URL MPP_SA_KEY MPP_SA_SECRET MPP_SA_PASSPHRASE \
                    MPP_SECRET_KEY MPP_REALM MPP_CURRENCY MPP_RECIPIENT MPP_ESCROW \
                    MPP_MERCHANT_PRIVATE_KEY"
                );
                std::process::exit(1);
            }
        };
        let pk_hex = std::env::var("MPP_MERCHANT_PRIVATE_KEY").unwrap_or_else(|_| {
            eprintln!("missing env var: MPP_MERCHANT_PRIVATE_KEY");
            eprintln!("(payee signer for SettleAuthorization/CloseAuthorization, must match MPP_RECIPIENT)");
            std::process::exit(1);
        });
        let signer: PrivateKeySigner = pk_hex.parse().unwrap_or_else(|e| {
            eprintln!("invalid MPP_MERCHANT_PRIVATE_KEY: {e}");
            std::process::exit(1);
        });
        let signer_addr = format!("{:#x}", signer.address());
        if signer_addr.to_lowercase() != cfg.recipient.to_lowercase() {
            eprintln!(
                "MPP_MERCHANT_PRIVATE_KEY address ({signer_addr}) does not match \
                MPP_RECIPIENT ({})",
                cfg.recipient
            );
            std::process::exit(1);
        }
        let client: Arc<dyn SaApiClient> = Arc::new(OkxSaApiClient::with_base_url(
            cfg.sa_url.clone(),
            cfg.sa_key.clone(),
            cfg.sa_secret.clone(),
            cfg.sa_passphrase.clone(),
        ));
        (client, cfg, signer)
    }
}

fn load_config() -> Result<Config, &'static str> {
    Ok(Config {
        sa_url: std::env::var("MPP_SA_URL").map_err(|_| "MPP_SA_URL")?,
        sa_key: std::env::var("MPP_SA_KEY").map_err(|_| "MPP_SA_KEY")?,
        sa_secret: std::env::var("MPP_SA_SECRET").map_err(|_| "MPP_SA_SECRET")?,
        sa_passphrase: std::env::var("MPP_SA_PASSPHRASE").map_err(|_| "MPP_SA_PASSPHRASE")?,
        secret_key: std::env::var("MPP_SECRET_KEY").map_err(|_| "MPP_SECRET_KEY")?,
        realm: std::env::var("MPP_REALM").map_err(|_| "MPP_REALM")?,
        currency: std::env::var("MPP_CURRENCY").map_err(|_| "MPP_CURRENCY")?,
        recipient: std::env::var("MPP_RECIPIENT").map_err(|_| "MPP_RECIPIENT")?,
        escrow: std::env::var("MPP_ESCROW").map_err(|_| "MPP_ESCROW")?,
    })
}

async fn manage(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    // With credential → verify & route.
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth.to_str() {
            match parse_authorization(auth_str) {
                Ok(credential) => {
                    let action = credential
                        .payload
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let request: mpp::protocol::intents::SessionRequest = match credential
                        .challenge
                        .request
                        .decode()
                    {
                        Ok(r) => r,
                        Err(e) => {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error": format!("decode request: {e}")})),
                            )
                                .into_response();
                        }
                    };
                    match state.session_method.verify_session(&credential, &request).await {
                        Ok(receipt) => {
                            if let Some(body) = state.session_method.respond(&credential, &receipt)
                            {
                                // Management action (open/topUp/close) — return receipt JSON.
                                return (StatusCode::OK, Json(body)).into_response();
                            }
                            // Voucher: 1 unit of business payload.
                            let channel_id = credential
                                .payload
                                .get("channelId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            return (
                                StatusCode::OK,
                                Json(json!({
                                    "service": "one unit delivered",
                                    "channelId": channel_id,
                                    "action": action,
                                })),
                            )
                                .into_response();
                        }
                        Err(e) => {
                            return (
                                StatusCode::PAYMENT_REQUIRED,
                                Json(json!({ "error": e.to_string() })),
                            )
                                .into_response();
                        }
                    }
                }
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("parse auth: {e}") })),
                    )
                        .into_response();
                }
            }
        }
    }

    // No credential — issue a 402 session challenge.
    let details = SessionMethodDetails {
        chain_id: 196,
        escrow_contract: state.escrow.clone(),
        channel_id: None,
        min_voucher_delta: Some("0".into()),
        fee_payer: Some(true),
        splits: None,
    };
    let request = match session_request_with(
        UNIT_PRICE_BASE_UNITS,
        &state.currency,
        &state.recipient,
        details,
    ) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("build session request: {e}")})),
            )
                .into_response()
        }
    };
    // Add the suggestedDeposit hint.
    let mut request = request;
    request.suggested_deposit = Some(SUGGESTED_DEPOSIT.into());
    request.unit_type = Some(UNIT_TYPE.into());

    let challenge = match build_session_challenge(
        &state.secret_key,
        &state.realm,
        &request,
        None,
        Some("Pay-per-request session"),
    ) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("build challenge: {e}") })),
            )
                .into_response();
        }
    };

    match format_www_authenticate(&challenge) {
        Ok(www) => (
            StatusCode::PAYMENT_REQUIRED,
            [(header::WWW_AUTHENTICATE, www)],
            Json(json!({
                "error": "Payment Required",
                "unitPrice": UNIT_PRICE_BASE_UNITS,
                "unitType": UNIT_TYPE,
                "suggestedDeposit": SUGGESTED_DEPOSIT,
                "escrow": state.escrow,
                "chainId": 196,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SettleBody {
    #[serde(rename = "channelId")]
    channel_id: String,
}

async fn settle(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SettleBody>,
) -> impl IntoResponse {
    match state
        .session_method
        .settle_with_authorization(&body.channel_id)
        .await
    {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct StatusQuery {
    #[serde(rename = "channelId")]
    channel_id: String,
}

async fn status(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> impl IntoResponse {
    match state.session_method.status(&q.channel_id).await {
        Ok(s) => (StatusCode::OK, Json::<ChannelStatus>(s)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Silence an unused-import warning for Value.
#[allow(dead_code)]
fn _force_link(v: Value) -> axum::response::Response {
    Json(v).into_response()
}
