//! MPP EVM Photo Server —— **分账版 + 调试日志**。
//!
//! 跟 `mpp_photo_server` 同流程，区别：
//! 1. 在 `EvmChargeChallengerConfig` 里配了 `splits: Some(vec![...])`，challenge 下发时会
//!    把 `ChargeMethodDetails.splits` 一起带出去。
//! 2. 绕开 `MppCharge<C>` extractor，手工调 `challenger.verify_payment()`，这样能把 SA API /
//!    签名验证失败的原始错误字符串打出来（extractor 会吞掉：`axum.rs:382 Err(_) =>`）。
//! 3. 加了一个 `dump_io` middleware 打印所有请求 / 响应的 method / path / headers / body，
//!    便于定位 403 / 402 / 签名失败等问题。
//!
//! 对应协议位置：mpp-specs draft-evm-charge-00.md §8.1 `ChargeMethodDetails.splits`，
//! §8.2 `Eip3009Authorization.splits`（独立签名）。
//!
//! # 分账约束（规范硬性要求）
//!
//! - `sum(splits[].amount) < request.amount`：primary recipient 必须保留非零余额。
//! - 每个 `ChargeSplit.amount` 是 base units 整数字符串（跟 `ChargeConfig::amount()` 同约定）。
//! - 每个 `ChargeSplit.recipient` 是合法 40-hex EIP-55 地址（占位地址也要 40-hex 才能过校验）。
//!
//! 下例：总价 100 base units（0.0001 pathUSD）→ partner-a 30, partner-b 20, primary 剩 50。
//!
//! # Running
//!
//! ```bash
//! MPP_MOCK=1 cargo run --example mpp_photo_split_server
//! ```

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger, PaymentRequired, WithReceipt};
use mpp_evm::sa_client::SaApiClient;
use mpp_evm::{
    ChargeSplit, EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, MockSaApiClient,
    OkxSaApiClient,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// 价格 —— 总额 100 base units；primary 50 / partner-a 30 / partner-b 20。
// ---------------------------------------------------------------------------

const PHOTO_AMOUNT: &str = "100";
const PHOTO_DESCRIPTION: &str = "One photo (split across partners)";

// ---------------------------------------------------------------------------
// /photo —— 手工实现 402/200 流程，verify 失败时打印原始错误原因
// ---------------------------------------------------------------------------

async fn photo(
    State(challenger): State<Arc<dyn ChargeChallenger>>,
    headers: HeaderMap,
) -> Response {
    println!("\n========== [handler] GET /photo ==========");

    // 1. 读 Authorization header。注意：**原样整行传给 verify_payment**，不要
    //    剥 `Payment ` 前缀。upstream `parse_authorization()` 期望完整头部，
    //    自己 .get(8..) 去 strip（headers.rs:443），double-strip 会报
    //    `Expected 'Payment' scheme`。
    let auth_raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_raw {
        None => {
            println!("[handler] no Authorization header → 发 challenge (402)");
            build_challenge_response(challenger.as_ref(), "missing-credential")
        }
        Some(header_val) => {
            let trimmed = header_val.trim();
            println!("[handler] Authorization header ({} chars, head 80): {}",
                trimmed.len(),
                &trimmed.chars().take(80).collect::<String>());
            if !trimmed.to_ascii_lowercase().starts_with("payment ") {
                println!("[handler] ❌ scheme != Payment, 拒掉");
                return build_challenge_response(challenger.as_ref(), "wrong-scheme");
            }
            println!("[handler] → challenger.verify_payment(...)");
            match challenger.verify_payment(trimmed).await {
                Ok(receipt) => {
                    println!("[handler] ✅ verify ok: method={} reference={} status={}",
                        receipt.method.as_str(), receipt.reference, receipt.status);
                    WithReceipt {
                        receipt,
                        body: Json(json!({ "url": "https://picsum.photos/id/42/1024/1024.jpg" })),
                    }
                    .into_response()
                }
                Err(e) => {
                    println!("[handler] ❌ verify FAILED: {e}");
                    build_challenge_response(challenger.as_ref(), &format!("verify-failed: {e}"))
                }
            }
        }
    }
}

/// 构造 402 响应 + 打印 challenge 内容。`note` 仅用于日志，不进响应。
fn build_challenge_response(challenger: &dyn ChargeChallenger, note: &str) -> Response {
    let options = ChallengeOptions {
        description: Some(PHOTO_DESCRIPTION),
    };
    match challenger.challenge(PHOTO_AMOUNT, options) {
        Ok(ch) => {
            println!("[handler] challenge ok (note={note})");
            println!("  id      = {}", ch.id);
            println!("  realm   = {}", ch.realm);
            println!("  method  = {}", ch.method.as_str());
            println!("  intent  = {}", ch.intent.as_str());
            println!("  request = {}", ch.request.raw());
            println!("  expires = {:?}", ch.expires);
            // decode 出来再打一次 body，含 splits，便于肉眼核对
            if let Ok(pretty) = base64_decode_json_pretty(ch.request.raw()) {
                println!("  decoded request:\n{pretty}");
            }
            PaymentRequired(ch).into_response()
        }
        Err(e) => {
            println!("[handler] challenge build error: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("challenge: {e}")).into_response()
        }
    }
}

fn base64_decode_json_pretty(b64url: &str) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64url.as_bytes())
        .map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

async fn health() -> Json<Value> {
    println!("\n========== [handler] GET /health ==========");
    Json(json!({ "status": "ok" }))
}

// ---------------------------------------------------------------------------
// middleware —— dump 请求 / 响应的 method / path / headers / body
// ---------------------------------------------------------------------------

async fn dump_io(req: Request<Body>, next: Next) -> Response {
    // ---- request ----
    let method = req.method().clone();
    let uri = req.uri().clone();
    println!("\n---------- [in] {method} {uri} ----------");
    dump_headers("req", req.headers());

    let (parts, body) = req.into_parts();
    let req_bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            println!("[in] body read error: {e}");
            Bytes::new()
        }
    };
    dump_body("req", &req_bytes);
    let req = Request::from_parts(parts, Body::from(req_bytes));

    // ---- run inner ----
    let resp = next.run(req).await;

    // ---- response ----
    println!("\n---------- [out] {method} {uri} → {} ----------", resp.status());
    dump_headers("resp", resp.headers());

    let (parts, body) = resp.into_parts();
    let resp_bytes = match axum::body::to_bytes(body, 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            println!("[out] body read error: {e}");
            Bytes::new()
        }
    };
    dump_body("resp", &resp_bytes);
    Response::from_parts(parts, Body::from(resp_bytes))
}

fn dump_headers(tag: &str, headers: &HeaderMap) {
    if headers.is_empty() {
        println!("[{tag}] headers: <empty>");
        return;
    }
    println!("[{tag}] headers:");
    for (k, v) in headers {
        let vs = v.to_str().unwrap_or("<non-ascii>");
        // 敏感 header 简单打码，避免 log 泄露
        let masked = match k.as_str().to_ascii_lowercase().as_str() {
            "authorization" | "cookie" | "x-api-key" => mask_secret(vs),
            _ => vs.to_string(),
        };
        println!("  {}: {}", k, masked);
    }
}

fn mask_secret(s: &str) -> String {
    // 保留前 8 / 后 4 字符，中间压成 ****，方便核对头尾但不泄露全串
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 16 {
        return "****".into();
    }
    let head: String = chars.iter().take(8).collect();
    let tail: String = chars.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}****{tail} (len={})", s.len())
}

fn dump_body(tag: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        println!("[{tag}] body: <empty>");
        return;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) if s.len() <= 4096 => println!("[{tag}] body ({} bytes): {s}", bytes.len()),
        Ok(s) => println!("[{tag}] body ({} bytes, head 4KB):\n{}", bytes.len(), &s[..4096]),
        Err(_) => println!("[{tag}] body: <{} non-utf8 bytes>", bytes.len()),
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // 默认开 mpp_evm=debug，这样 SA API 的 request/response body 会被 tracing 打出来。
    // 用户也可以通过 RUST_LOG 覆盖（例如 `RUST_LOG=mpp_evm=trace,debug`）。
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,mpp_evm=debug".parse().unwrap());
    tracing_subscriber::fmt().with_env_filter(filter).init();
    println!("=== MPP EVM Photo Server (with splits, verbose) ===\n");

    let env = load_env();
    let sa_client = env.build_sa_client();

    // 关键点：splits 放在服务级配置里，一次性配好，challenge() 会把它带进
    // ChargeMethodDetails.splits 下发给客户端。
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
        .layer(middleware::from_fn(dump_io))
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

// ---------------------------------------------------------------------------
// 环境装载
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
    split_a_recipient: String,
    split_b_recipient: String,
    mock: bool,
}

impl Env {
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

fn load_env() -> Env {
    if std::env::var("MPP_MOCK").ok().as_deref() == Some("1") {
        println!("⚠ MPP_MOCK=1 — using MockSaApiClient, no real SA API calls");
        Env {
            sa_url: "http://mock.local".into(),
            sa_key: "mock".into(),
            sa_secret: "mock".into(),
            sa_passphrase: "mock".into(),
            secret_key: "mock-hmac-secret".into(),
            realm: "mock.local".into(),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            split_a_recipient: "0x1111111111111111111111111111111111111111".into(),
            split_b_recipient: "0x2222222222222222222222222222222222222222".into(),
            mock: true,
        }
    } else {
        let required = |k: &str| {
            std::env::var(k).unwrap_or_else(|_| {
                eprintln!("missing env var: {k}");
                eprintln!("tip: set MPP_MOCK=1 to run with mocked SA API (no creds needed)");
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
            mock: false,
        }
    }
}
