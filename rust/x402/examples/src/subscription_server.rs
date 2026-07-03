//! Minimal Axum server demoing x402 `period` subscriptions: subscribe,
//! access via `APP-Access`, upgrade/downgrade, cancel, and merchant access-deny.
//!
//! Run: `cargo run --example subscription_server`
//!
//! Requires environment variables:
//!   OKX_API_KEY, OKX_SECRET_KEY, OKX_PASSPHRASE, PAY_TO_ADDRESS
//!   (SUBSCRIPTION_CONTRACT optional — else sourced from /supported)

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use x402_axum::{
    AccessContext, BeforeAccessResult, InMemorySubscriptionStore, OnBeforeAccessHook,
    PaymentMiddlewareBuilder, RoutePaymentConfig, SubscriptionSupport,
};
use x402_core::http::{OkxHttpFacilitatorClient, SubscriptionOperation};
use x402_core::server::X402ResourceServer;
use x402_evm::subscription::{
    PermitSubscriptionScheme, SubscriptionPlan, DEFAULT_ACCESS_PROOF_WINDOW_SECS,
};

/// Current Unix time in seconds.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    // INFO by default; override with `RUST_LOG`.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let api_key = std::env::var("OKX_API_KEY").expect("OKX_API_KEY is required");
    let secret_key = std::env::var("OKX_SECRET_KEY").expect("OKX_SECRET_KEY is required");
    let passphrase = std::env::var("OKX_PASSPHRASE").expect("OKX_PASSPHRASE is required");
    let pay_to = std::env::var("PAY_TO_ADDRESS").expect("PAY_TO_ADDRESS is required");
    // Optional A2APaySubscription contract override; else from /supported.
    let subscription_contract = std::env::var("SUBSCRIPTION_CONTRACT").ok();
    // Optional non-prod facilitator; unset → default https://web3.okx.com.
    let facilitator_url = std::env::var("FACILITATOR_URL").ok();

    // Build a facilitator client. `OkxHttpFacilitatorClient` implements both
    // `FacilitatorClient` and `SubscriptionFacilitatorClient`; the resource
    // server and `SubscriptionSupport` each take their own instance.
    let make_client = || {
        let client = match &facilitator_url {
            Some(url) => {
                OkxHttpFacilitatorClient::with_url(url, &api_key, &secret_key, &passphrase)
            }
            None => OkxHttpFacilitatorClient::new(&api_key, &secret_key, &passphrase),
        };
        client.expect("failed to create facilitator client")
    };

    // 1. Resource server: register the subscription scheme.
    //    `with_subscription_contract` pins the contract address.
    let mut scheme = PermitSubscriptionScheme::new();
    if let Some(addr) = subscription_contract {
        scheme = scheme.with_subscription_contract(addr);
    }
    let mut server = X402ResourceServer::new(make_client()).register("eip155:196", scheme);
    server
        .initialize()
        .await
        .expect("failed to initialize: check facilitator connectivity");

    // 2. Subscription support: create/charge/get_subscription go through this
    //    client (AccessProof replay window ±300s). The in-memory store does
    //    write-after-sync plus a 30s access cache to skip repeat round-trips.
    let store = Arc::new(InMemorySubscriptionStore::new());

    // Merchant access policy for `on_before_access`: an in-memory set of subIds
    // the merchant denies access to. Use when the seller has canceled a
    // subscription and also wants to cut off the buyer's remaining paid-period
    // access — the hook refuses these subIds before the period gate (hard 401)
    // instead of letting the paid period run to its end. Access-deny only;
    // billing stops via the on-chain cancel.
    let denied_subs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let denied_for_hook = denied_subs.clone();
    let on_before_access: OnBeforeAccessHook = Arc::new(move |ctx: AccessContext| {
        let denied = denied_for_hook.clone();
        Box::pin(async move {
            // Read the bool and drop the lock before any `.await`.
            let is_denied = denied.lock().map(|s| s.contains(&ctx.sub_id)).unwrap_or(false);
            if is_denied {
                BeforeAccessResult {
                    abort: true,
                    reason: Some(format!("access denied by merchant for subscription {}", ctx.sub_id)),
                }
            } else {
                BeforeAccessResult::default()
            }
        })
    });

    let subscription = SubscriptionSupport::new(
        Arc::new(make_client()),
        DEFAULT_ACCESS_PROOF_WINDOW_SECS,
    )
    .with_store(store.clone())
    .on_before_access(on_before_access);

    // 3. Declare the plans offered on the resource routes. One route can carry
    //    multiple plans (a menu); a subscription is admitted only if its planId
    //    is among the route's accepted plan ids.
    let basic = SubscriptionPlan {
        id: "basic_monthly".into(),
        tier: 1,
        network: "eip155:196".into(),
        pay_to: pay_to.clone(),
        price: "$0.005".into(),
        amount_per_period: "5000".into(),
        period_sec: 2_592_000, // 30 days
        period_mode: 0,        // fixed_seconds
        max_periods: 12,
        start_at: 0,
        initial_charge_periods: 1,
        initial_charge_amount: "5000".into(),
        max_timeout_seconds: Some(600),
        name: Some("Basic Monthly".into()),
        features: Some(vec!["api_basic".into()]),
    };
    let pro = SubscriptionPlan {
        id: "pro_monthly".into(),
        tier: 2,
        price: "$0.02".into(),
        amount_per_period: "20000".into(),
        initial_charge_amount: "20000".into(),
        name: Some("Pro Monthly".into()),
        features: Some(vec!["api_basic".into(), "api_pro".into()]),
        ..basic.clone()
    };
    // Yearly Pro: cheaper per-period, whole year prepaid (initialCharge covers
    // all 12 periods). Tiers must be unique across plans — change-plan derives
    // direction from tier and the contract rejects a same-tier change.
    let pro_yearly = SubscriptionPlan {
        id: "pro_yearly".into(),
        tier: 3,
        price: "$0.192".into(),
        amount_per_period: "16000".into(),
        initial_charge_periods: 12,
        initial_charge_amount: "192000".into(),
        name: Some("Pro Yearly".into()),
        features: Some(vec!["api_basic".into(), "api_pro".into()]),
        ..basic.clone()
    };

    let routes = HashMap::from([
        (
            // Basic resource: only the basic_monthly plan (tier 1) is accepted.
            "GET /weather".to_string(),
            RoutePaymentConfig {
                accepts: vec![basic.to_accept_config()],
                description: "Weather data (Basic plan)".into(),
                mime_type: "application/json".into(),
                sync_settle: Some(true),
                resource: None,
                operation: None,
            },
        ),
        (
            // Premium resource: only the Pro plans (tier 2 / 3) are accepted.
            // A basic_monthly subscriber is gated out with 402 (`plan not accepted`).
            "GET /premium".to_string(),
            RoutePaymentConfig {
                accepts: vec![pro.to_accept_config(), pro_yearly.to_accept_config()],
                description: "Premium weather analytics (Pro plans only)".into(),
                mime_type: "application/json".into(),
                sync_settle: Some(true),
                resource: None,
                operation: None,
            },
        ),
        (
            // Single plan-change endpoint for ALL plans (basic + both pro tiers),
            // not tied to one resource. Hit with an `APP-Access` proof to get the
            // plans you can switch to (each annotated with `extra.changeFrom`).
            // Sign the chosen one and resend with `PAYMENT-SIGNATURE` to execute.
            "GET /subscription/change".to_string(),
            RoutePaymentConfig {
                accepts: vec![
                    basic.to_accept_config(),
                    pro.to_accept_config(),
                    pro_yearly.to_accept_config(),
                ],
                description: "Change your subscription plan".into(),
                mime_type: "application/json".into(),
                sync_settle: Some(true),
                resource: None,
                operation: Some(SubscriptionOperation::Change),
            },
        ),
        (
            // Cancel: buyer POSTs a signed `CancelAuth`; the middleware relays it
            // to the facilitator (no 402 — a signed-auth relay, not a payment).
            "POST /subscription/cancel".to_string(),
            RoutePaymentConfig {
                accepts: vec![],
                description: "Cancel a subscription".into(),
                mime_type: "application/json".into(),
                sync_settle: Some(true),
                resource: None,
                operation: Some(SubscriptionOperation::Cancel),
            },
        ),
        (
            // Revert a not-yet-effective downgrade: buyer POSTs a signed
            // `PendingChangeCancelAuth`; the middleware relays it.
            "POST /subscription/cancel-pending".to_string(),
            RoutePaymentConfig {
                accepts: vec![],
                description: "Cancel a scheduled downgrade".into(),
                mime_type: "application/json".into(),
                sync_settle: Some(true),
                resource: None,
                operation: Some(SubscriptionOperation::CancelPendingChange),
            },
        ),
    ]);

    // 4. Build the middleware with subscription enabled. Cancel / cancel-pending
    //    are declared as `operation` routes above; the middleware relays the
    //    buyer-signed auth to the facilitator (no manual handler needed).
    let denied_route = denied_subs.clone();

    let app = Router::new()
        .route("/weather", get(weather_handler))
        .route("/premium", get(weather_handler))
        .route("/subscription/change", get(change_handler))
        // Demo the `on_before_access` policy: deny a subId's access (e.g. after a
        // seller cancel) so subsequent APP-Access to it is refused immediately.
        .route(
            "/subscription/merchant-deny-access",
            post(move |body: Json<Value>| {
                let denied = denied_route.clone();
                async move { merchant_deny_handler(denied, body.0).await }
            }),
        )
        .layer(
            PaymentMiddlewareBuilder::new(routes, server)
                .subscription(subscription.clone())
                .build(),
        );

    // 5. Background charge scheduler: every 60s, drain the store's due list and
    //    charge each due subscription. `charge_and_record` reconciles the store
    //    from each result — advances `next_chargeable_at` on a normal charge and
    //    migrates to the successor subId when a scheduled downgrade activates.
    //    `next_chargeable_at` is seeded at subscribe, so charging runs on
    //    schedule without waiting for a first access.
    let scheduler = subscription;
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            for rec in scheduler.due_subscriptions(now_unix()).await {
                match scheduler.charge_and_record(&rec.sub_id, true).await {
                    Ok(o) if o.plan_change_triggered => println!(
                        "charged {} → downgrade activated → now {}",
                        rec.sub_id,
                        o.new_sub_id.as_deref().unwrap_or("?")
                    ),
                    Ok(o) => println!(
                        "charged {} → period {} state {} tx {}",
                        rec.sub_id, o.period, o.state, o.tx_hash.as_deref().unwrap_or("-")
                    ),
                    // Dunning hook: classify `e` and apply a retry / notify /
                    // auto-cancel policy here. Demo just logs.
                    Err(e) => eprintln!("charge {} failed (dunning): {e}", rec.sub_id),
                }
            }
        }
    });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4022").await.unwrap();
    println!("Subscription server listening at http://localhost:4022");
    println!("  basic:       curl http://localhost:4022/weather            (→ 402, accepts: basic_monthly only)");
    println!("  premium:     curl http://localhost:4022/premium            (→ 402, accepts: pro_monthly / pro_yearly — basic sub is gated out with 402)");
    println!("  change:      curl -H 'APP-Access: <proof>' http://localhost:4022/subscription/change  (→ 402 with changeFrom per plan)");
    println!("  cancel:          POST http://localhost:4022/subscription/cancel          (body = signed CancelAuth)");
    println!("  cancel-pending:  POST http://localhost:4022/subscription/cancel-pending");
    println!("  deny-access:     POST http://localhost:4022/subscription/merchant-deny-access  (body = {{\"subId\":\"0x…\"}} → on_before_access refuses it immediately)");
    axum::serve(listener, app).await.unwrap();
}

/// Deny a subId's access so subsequent `APP-Access` is refused immediately (hard
/// 401 before the period gate). Body: `{ "subId": "0x…" }`.
async fn merchant_deny_handler(
    denied: Arc<Mutex<HashSet<String>>>,
    body: Value,
) -> (StatusCode, Json<Value>) {
    let sub_id = body
        .get("subId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if sub_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "subId required" })));
    }
    denied.lock().unwrap().insert(sub_id.clone());
    (StatusCode::OK, Json(json!({ "denied": sub_id })))
}

async fn weather_handler() -> Json<Value> {
    Json(json!({
        "report": { "weather": "sunny", "temperature": 23 }
    }))
}

/// Served only after a successful plan change (settle-before-serve). The
/// authoritative result — subId / txHash / state — is in the `PAYMENT-RESPONSE`
/// header the middleware adds; this body is just a confirmation.
async fn change_handler() -> Json<Value> {
    Json(json!({ "result": "subscription plan changed" }))
}
