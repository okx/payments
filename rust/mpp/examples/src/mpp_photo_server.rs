//! MPP EVM Payment-Gated Photo Server —— ergonomic extractor 版。
//!
//! 用 upstream `MppCharge<C>` extractor + `WithReceipt<T>` 响应包装 + 我们的
//! `EvmChargeChallenger`（`impl ChargeChallenger`），彻底省掉手写 parse/verify/
//! format_www_authenticate 的样板。整个 server 一屏就写完。
//!
//! 流程（对外 HTTP 行为跟旧版完全等价）：
//! 1. `GET /photo` 无 Authorization → 402 + `WWW-Authenticate: Payment ...`
//! 2. 客户端签 EIP-3009 → 带 `Authorization: Payment <base64url>` 重试
//! 3. 服务端通过 SA API 验签 + 扣费 → 200 + `Payment-Receipt` header + 照片 URL
//!
//! # Running
//!
//! **本地 dev / onchainos 联调** (无需真凭证)：
//!
//! ```bash
//! MPP_MOCK=1 cargo run --example mpp_photo_server
//! ```
//!
//! **真实 SA API**：
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
use mpp_evm::{
    EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, MockSaApiClient,
    OkxSaApiClient,
};
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// 每路由价格 —— per-route config, 由 MppCharge<C> 在每次请求时拿 C::amount()
// ---------------------------------------------------------------------------

/// 100 base units of pathUSD (6 decimals) = 0.0001 pathUSD。
///
/// amount() 必须是 base units 整数字符串 —— MPP 协议规范强制要求 request 里的 amount
/// 是 "base-10 integer string with no sign, decimal point, exponent"。不要写 "0.0001"
/// 那种 dollar 风格（那是 upstream Tempo 后端内部做了转换才能用的, 协议规范本身不允许）。
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
// 业务 handler —— 付费验签过后 MppCharge<OnePhoto> 才会提取成功。
// 用 WithReceipt 包装响应, 自动挂 Payment-Receipt header。
// ---------------------------------------------------------------------------

async fn photo(charge: MppCharge<OnePhoto>) -> WithReceipt<Json<Value>> {
    // 固定返回一个示例 URL, 不发外网请求 (便于离线 / onchainos 联调环境跑通)
    WithReceipt {
        receipt: charge.receipt,
        body: Json(json!({ "url": "https://picsum.photos/id/42/1024/1024.jpg" })),
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

// ---------------------------------------------------------------------------
// main: 装 challenger -> 挂到 axum state -> 起服务
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    println!("=== MPP EVM Photo Server ===\n");

    let env = load_env();
    let sa_client = env.build_sa_client();
    let challenger: Arc<dyn ChargeChallenger> = Arc::new(EvmChargeChallenger::new(
        EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(sa_client),
            currency: env.currency.clone(),
            recipient: env.recipient.clone(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: env.realm.clone(),
            secret_key: env.secret_key,
            splits: None,
        },
    ));

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
// 环境装载 —— MPP_MOCK=1 模式给 MockSaApiClient + 合法 40-hex 占位地址
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
            // 合法 40-hex 占位地址 (真实 X Layer pathUSD + 测试 recipient), 客户端地址校验能过
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            mock: true,
        }
    } else {
        let required = |k: &str| {
            std::env::var(k).unwrap_or_else(|_| {
                eprintln!("missing env var: {k}");
                eprintln!("tip: set MPP_MOCK=1 to run with mocked SA API (no creds needed)");
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
            mock: false,
        }
    }
}

