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
use mpp_evm::charge::challenge::{build_session_challenge, session_request_with};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::types::{ChannelStatus, SessionMethodDetails};
use mpp_evm::{CredentialExt, EvmSessionMethod, OkxSaApiClient};
use alloy_primitives::Address;
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

    let (sa_client, cfg, signer) = load_client_and_config();

    let expected_payee: Address = cfg.recipient.parse().unwrap_or_else(|e| {
        eprintln!("invalid MPP_RECIPIENT address {}: {e}", cfg.recipient);
        std::process::exit(1);
    });
    let session_method = Arc::new(
        EvmSessionMethod::new(sa_client)
            .with_escrow(&cfg.escrow)
            .with_signer(signer)
            .verify_payee(expected_payee)
            .unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1);
            }),
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

/// 装载 SA API client + Config + payee Signer。
///
/// 所有 env var 都必填(包括 `MPP_MERCHANT_PRIVATE_KEY`,必须与 `MPP_RECIPIENT` 同地址)。
fn load_client_and_config() -> (Arc<dyn SaApiClient>, Config, PrivateKeySigner) {
    let cfg = match load_config() {
        Ok(c) => c,
        Err(missing) => {
            eprintln!("missing env var: {missing}");
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
    // signer/recipient mismatch 在 main 用 EvmSessionMethod::verify_payee fast-fail。
    let client: Arc<dyn SaApiClient> = Arc::new(OkxSaApiClient::with_base_url(
        cfg.sa_url.clone(),
        cfg.sa_key.clone(),
        cfg.sa_secret.clone(),
        cfg.sa_passphrase.clone(),
    ));
    (client, cfg, signer)
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
                    let request: mpp::protocol::intents::SessionRequest =
                        match credential.decode_request() {
                            Ok(r) => r,
                            Err(e) => {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(json!({ "error": e.to_string() })),
                                )
                                    .into_response();
                            }
                        };
                    match state.session_method.verify_session(&credential, &request).await {
                        Ok(receipt) => {
                            let respond_body =
                                state.session_method.respond(&credential, &receipt);
                            // Management action (open / topUp / close): only respond body.
                            // Voucher: respond body 含 spent/units,需与商户业务负载合并。
                            if action != "voucher" {
                                if let Some(body) = respond_body {
                                    return (StatusCode::OK, Json(body)).into_response();
                                }
                            }
                            // Voucher branch: merge spent/units(if any)with business payload.
                            let channel_id = credential
                                .payload
                                .get("channelId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let mut body = json!({
                                "service": "one unit delivered",
                                "channelId": channel_id,
                                "action": action,
                            });
                            if let Some(serde_json::Value::Object(map)) = respond_body {
                                let body_obj = body.as_object_mut().unwrap();
                                for (k, v) in map {
                                    // 不覆盖业务字段,只补缺失的 spent / units 等
                                    body_obj.entry(k).or_insert(v);
                                }
                            }
                            return (StatusCode::OK, Json(body)).into_response();
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
    // MPP_FEE_PAYER 控制 challenge 走哪种 open 模式:
    //   true (默认):seller 兜底 broadcast,客户端发 EIP-3009 → transaction mode
    //   false:客户端自己上链 broadcast,只回报 tx hash → hash mode
    let fee_payer = std::env::var("MPP_FEE_PAYER")
        .ok()
        .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
        .unwrap_or(true);
    let details = SessionMethodDetails {
        chain_id: 196,
        escrow_contract: state.escrow.clone(),
        channel_id: None,
        min_voucher_delta: Some("0".into()),
        fee_payer: Some(fee_payer),
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
