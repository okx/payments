//! Valid payer proofs must authorize only the merchant selected for this request.
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::{eip191_hash_message, keccak256, Address, B256, U256};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use k256::ecdsa::SigningKey;
use serde_json::json;
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};
use x402_axum::{
    AcceptConfig, InMemorySubscriptionStore, PaymentMiddlewareBuilder, ResolvedAccept,
    RoutePaymentConfig, SubscriptionRecord, SubscriptionStore, SubscriptionSupport,
};
use x402_core::{http::OkxHttpFacilitatorClient, server::X402ResourceServer};

const SUB_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
const MERCHANT_A: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERCHANT_B: &str = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// Ephemeral OS-random signer. All HTTP traffic stays on the local mock server.
fn signed_proof(now: u64) -> (String, String) {
    let key = SigningKey::random(&mut k256::elliptic_curve::rand_core::OsRng);
    let public = key.verifying_key().to_encoded_point(false);
    let payer = Address::from_slice(&keccak256(&public.as_bytes()[1..])[12..]);
    let mut packed = Vec::new();
    packed.extend_from_slice(SUB_ID.parse::<B256>().unwrap().as_slice());
    packed.extend_from_slice(payer.as_slice());
    packed.extend_from_slice(&U256::from(now).to_be_bytes::<32>());
    let hash = eip191_hash_message(keccak256(packed));
    let (sig, recovery) = key.sign_prehash_recoverable(hash.as_slice()).unwrap();
    let mut bytes = sig.to_bytes().to_vec();
    bytes.push(27 + recovery.to_byte());
    let payer = format!("{payer:#x}");
    let proof = STANDARD.encode(
        serde_json::to_vec(&json!({
            "kind": "subscription-id", "subId": SUB_ID, "payer": payer,
            "timestamp": now, "signature": format!("0x{}", alloy_primitives::hex::encode(bytes)),
        }))
        .unwrap(),
    );
    (proof, payer)
}

async fn request(app: &Router, proof: &str, tenant: &str) -> StatusCode {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/content")
                .header("APP-Access", proof)
                .header("x-tenant", tenant)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if status == StatusCode::PAYMENT_REQUIRED {
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let message = String::from_utf8_lossy(&body);
        assert!(
            message.contains("merchant"),
            "expected merchant denial, got {message}"
        );
    }
    status
}

#[tokio::test]
async fn dynamic_merchant_is_bound_on_cold_warm_and_legacy_cache_access() {
    for legacy in [false, true] {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let (proof, payer) = signed_proof(now);
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v6/pay/x402/subscriptions/detail"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "data": {"subId": SUB_ID, "payer": payer, "merchant": MERCHANT_A,
                    "state": 1, "planId": "pro", "planTier": 2, "periodSec": 3600,
                    "startAt": now - 10, "elapsedPeriods": 1, "lastChargedPeriod": 1}
            })))
            .expect(1)
            .mount(&mock)
            .await;
        let store = Arc::new(InMemorySubscriptionStore::new());
        if legacy {
            // The pre-fix JSON schema is intentionally missing merchant.
            let record: SubscriptionRecord = serde_json::from_value(json!({
                "subId": SUB_ID, "state": 1, "payer": payer, "planId": "pro", "planTier": 2,
                "startAt": now - 10, "periodSec": 3600, "lastChargedPeriod": 1, "updatedAt": now
            }))
            .unwrap();
            assert!(record.merchant.is_empty());
            store.put(record).await;
        }
        let client = OkxHttpFacilitatorClient::with_url(&mock.uri(), "", "", "").unwrap();
        let support = SubscriptionSupport::new(
            Arc::new(OkxHttpFacilitatorClient::with_url(&mock.uri(), "", "", "").unwrap()),
            300,
        )
        .with_store(store.clone());
        let routes = HashMap::from([(
            "GET /content".to_string(),
            RoutePaymentConfig {
                accepts: vec![AcceptConfig {
                    scheme: "period".into(),
                    price: "0".into(),
                    network: "eip155:196".into(),
                    pay_to: MERCHANT_A.into(),
                    max_timeout_seconds: None,
                    extra: Some(HashMap::from([(
                        "plan".into(),
                        json!({"id": "pro", "tier": 2}),
                    )])),
                }],
                description: String::new(),
                mime_type: "application/json".into(),
                sync_settle: None,
                resource: None,
                operation: None,
            },
        )]);
        let calls = Arc::new(AtomicUsize::new(0));
        let resolver_calls = calls.clone();
        let layer = PaymentMiddlewareBuilder::new(routes, X402ResourceServer::new(client))
            .subscription(support)
            .resolver(Box::new(move |context, accept| {
                resolver_calls.fetch_add(1, Ordering::SeqCst);
                let pay_to = if context.headers.get("x-tenant").map(String::as_str) == Some("B") {
                    MERCHANT_B
                } else {
                    MERCHANT_A
                };
                let resolved = ResolvedAccept {
                    scheme: accept.scheme.clone(),
                    price: accept.price.clone(),
                    network: accept.network.clone(),
                    pay_to: pay_to.into(),
                    max_timeout_seconds: accept.max_timeout_seconds,
                    extra: accept.extra.clone(),
                };
                Box::pin(async move { resolved })
            }))
            .build();
        let app = Router::new()
            .route("/content", get(|| async { "paid content" }))
            .layer(layer);
        assert_eq!(
            request(&app, &proof, "B").await,
            StatusCode::PAYMENT_REQUIRED
        );
        assert_eq!(store.get(SUB_ID).await.unwrap().merchant, MERCHANT_A);
        assert_eq!(request(&app, &proof, "A").await, StatusCode::OK);
        assert_eq!(
            request(&app, &proof, "B").await,
            StatusCode::PAYMENT_REQUIRED
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "resolve exactly once per request"
        );
    }
}

#[tokio::test]
async fn authoritative_status_without_merchant_cannot_authorize_access() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (proof, payer) = signed_proof(now);
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/subscriptions/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "data": {"subId": SUB_ID, "payer": payer, "state": 1,
                "planId": "pro", "elapsedPeriods": 1, "lastChargedPeriod": 1}
        })))
        .expect(1)
        .mount(&mock)
        .await;
    let client = OkxHttpFacilitatorClient::with_url(&mock.uri(), "", "", "").unwrap();
    let routes = HashMap::from([(
        "GET /content".into(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "period".into(),
                price: "0".into(),
                network: "eip155:196".into(),
                pay_to: MERCHANT_A.into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: String::new(),
            mime_type: "application/json".into(),
            sync_settle: None,
            resource: None,
            operation: None,
        },
    )]);
    let layer = PaymentMiddlewareBuilder::new(
        routes,
        X402ResourceServer::new(
            OkxHttpFacilitatorClient::with_url(&mock.uri(), "", "", "").unwrap(),
        ),
    )
    .subscription(SubscriptionSupport::new(Arc::new(client), 300))
    .build();
    let app = Router::new()
        .route("/content", get(|| async { "paid content" }))
        .layer(layer);
    assert_eq!(
        request(&app, &proof, "A").await,
        StatusCode::PAYMENT_REQUIRED
    );
}
