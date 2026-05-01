//! Integration tests for x402-axum payment middleware.
//!
//! Tests the full Axum middleware flow using wiremock as a mock facilitator.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use std::time::Duration;

use x402_axum::{
    payment_middleware, AcceptConfig, BeforeHookResult, PaymentMiddlewareBuilder,
    ProtectedRequestResult, RoutePaymentConfig, SettleRecoveryResult, SettlementTimeoutResult,
    VerifyRecoveryResult,
};
use x402_core::http::{encode_payment_signature_header, OkxHttpFacilitatorClient};
use x402_core::server::X402ResourceServer;
use x402_core::types::*;
use x402_evm::{AggrDeferredEvmScheme, ExactEvmScheme};

/// Mount the /supported mock so initialize() succeeds.
async fn mount_supported_mock(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kinds": [{ "x402Version": 2, "scheme": "exact", "network": "eip155:196" }],
            "extensions": [],
            "signers": { "eip155:*": ["0xFacilitator"] }
        })))
        .mount(mock_server)
        .await;
}

/// Helper: build a test app with payment middleware pointing at mock facilitator.
async fn build_test_app(mock_server: &MockServer) -> Router {
    // Ensure /supported is mounted for initialize()
    mount_supported_mock(mock_server).await;

    let facilitator =
        OkxHttpFacilitatorClient::with_url(&mock_server.uri(), "key", "secret", "pass")
            .expect("failed to create facilitator client");

    let mut server =
        X402ResourceServer::new(facilitator).register("eip155:196", ExactEvmScheme::new());

    server.initialize().await.expect("initialize failed");

    let routes = HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "exact".into(),
                price: "$0.001".into(),
                network: "eip155:196".into(),
                pay_to: "0xSeller".into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "Weather data".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        },
    )]);

    Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .route("/free", get(|| async { Json(json!({"status": "ok"})) }))
        .layer(payment_middleware(routes, server))
}

/// Helper: build a valid PaymentPayload for testing.
fn test_payment_payload() -> PaymentPayload {
    PaymentPayload {
        x402_version: 2,
        resource: None,
        accepted: PaymentRequirements {
            scheme: "exact".into(),
            network: "eip155:196".into(),
            asset: "0x779ded0c9e1022225f8e0630b35a9b54be713736".into(),
            amount: "1000".into(),
            pay_to: "0xSeller".into(),
            max_timeout_seconds: 60,
            extra: HashMap::new(),
        },
        payload: {
            let mut m = HashMap::new();
            m.insert("signature".into(), json!("0xabc"));
            m.insert(
                "authorization".into(),
                json!({
                    "from": "0xBuyer",
                    "to": "0xSeller",
                    "value": "1000",
                    "validAfter": "0",
                    "validBefore": "9999999999",
                    "nonce": "0x1234"
                }),
            );
            m
        },
        extensions: None,
    }
}

/// Mount standard facilitator mocks for /supported, /verify, /settle.
async fn mount_facilitator_mocks(
    mock_server: &MockServer,
    verify_valid: bool,
    settle_success: bool,
) {
    // /supported
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kinds": [{ "x402Version": 2, "scheme": "exact", "network": "eip155:196" }],
            "extensions": [],
            "signers": { "eip155:*": ["0xFacilitator"] }
        })))
        .mount(mock_server)
        .await;

    // /verify
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "isValid": verify_valid,
            "invalidReason": if verify_valid { Value::Null } else { json!("expired") },
            "payer": "0xBuyer"
        })))
        .mount(mock_server)
        .await;

    // /settle
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": settle_success,
            "payer": "0xBuyer",
            "transaction": if settle_success { "0xTxHash" } else { "" },
            "network": "eip155:196",
            "status": if settle_success { "success" } else { "failed" },
            "errorReason": if settle_success { Value::Null } else { json!("insufficient_funds") }
        })))
        .mount(mock_server)
        .await;
}

// ---------------------------------------------------------------------------
// Test: Free endpoint (no payment required)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_free_endpoint_no_payment_required() {
    let mock_server = MockServer::start().await;
    let app = build_test_app(&mock_server).await;

    let response = app
        .oneshot(Request::builder().uri("/free").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Test: Protected endpoint without payment → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_no_payment_returns_402() {
    let mock_server = MockServer::start().await;
    let app = build_test_app(&mock_server).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);

    // Should have PAYMENT-REQUIRED header
    assert!(response.headers().contains_key("payment-required"));
}

// ---------------------------------------------------------------------------
// Test: Valid payment → verify → settle → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_valid_payment_returns_200() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let app = build_test_app(&mock_server).await;
    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    // Should have PAYMENT-RESPONSE header after successful settlement
    assert!(response.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Test: Verify fails → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_verify_failure_returns_402() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, false, false).await;

    let app = build_test_app(&mock_server).await;
    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Test: Settle fails → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_settle_failure_returns_402() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, false).await;

    let app = build_test_app(&mock_server).await;
    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Test: Invalid payment header → 400
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_invalid_payment_header_returns_400() {
    let mock_server = MockServer::start().await;
    let app = build_test_app(&mock_server).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", "not-valid-base64!!!")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Helper: build test routes config (reused by hook tests)
// ---------------------------------------------------------------------------

fn test_routes() -> HashMap<String, RoutePaymentConfig> {
    HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "exact".into(),
                price: "$0.001".into(),
                network: "eip155:196".into(),
                pay_to: "0xSeller".into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "Weather data".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        },
    )])
}

/// Helper: build an initialized X402ResourceServer for hook tests.
async fn build_initialized_server(mock_server: &MockServer) -> X402ResourceServer {
    mount_supported_mock(mock_server).await;
    let facilitator =
        OkxHttpFacilitatorClient::with_url(&mock_server.uri(), "key", "secret", "pass")
            .expect("failed to create facilitator client");
    let mut server =
        X402ResourceServer::new(facilitator).register("eip155:196", ExactEvmScheme::new());
    server.initialize().await.expect("initialize failed");
    server
}

/// Helper: build an initialized X402ResourceServer with both exact and aggr_deferred.
async fn build_initialized_server_with_deferred(mock_server: &MockServer) -> X402ResourceServer {
    // Mount supported with both schemes
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kinds": [
                { "x402Version": 2, "scheme": "exact", "network": "eip155:196" },
                { "x402Version": 2, "scheme": "aggr_deferred", "network": "eip155:196" }
            ],
            "extensions": [],
            "signers": { "eip155:*": ["0xFacilitator"] }
        })))
        .mount(mock_server)
        .await;

    let facilitator =
        OkxHttpFacilitatorClient::with_url(&mock_server.uri(), "key", "secret", "pass")
            .expect("failed to create facilitator client");
    let mut server = X402ResourceServer::new(facilitator)
        .register("eip155:196", ExactEvmScheme::new())
        .register("eip155:196", AggrDeferredEvmScheme::new());
    server.initialize().await.expect("initialize failed");
    server
}

// ---------------------------------------------------------------------------
// Test: onProtectedRequest — grant_access bypasses payment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_protected_request_grant_access() {
    let mock_server = MockServer::start().await;
    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_protected_request(Box::new(|_ctx| {
            Box::pin(async move {
                ProtectedRequestResult {
                    grant_access: true,
                    abort: false,
                    reason: None,
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    // No payment header, but hook grants access
    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Test: onProtectedRequest — abort denies access
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_protected_request_abort() {
    let mock_server = MockServer::start().await;
    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_protected_request(Box::new(|_ctx| {
            Box::pin(async move {
                ProtectedRequestResult {
                    grant_access: false,
                    abort: true,
                    reason: Some("IP blocked".into()),
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Test: onBeforeVerify — abort stops verification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_before_verify_abort() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_before_verify(Box::new(|_ctx| {
            Box::pin(async move {
                BeforeHookResult {
                    abort: true,
                    reason: Some("spending limit exceeded".into()),
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Test: onVerifyFailure — recovered proceeds to settlement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_verify_failure_recovered() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, false, true).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_verify_failure(Box::new(|_ctx, _reason| {
            Box::pin(async move {
                Some(VerifyRecoveryResult {
                    recovered: true,
                    result: None,
                })
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Verify failed but hook recovered → settlement proceeds → 200
    assert_eq!(response.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Test: onBeforeSettle — abort stops settlement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_before_settle_abort() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_before_settle(Box::new(|_ctx| {
            Box::pin(async move {
                BeforeHookResult {
                    abort: true,
                    reason: Some("compliance check failed".into()),
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Test: onSettleFailure — recovered delivers resource
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_on_settle_failure_recovered() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, false).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_settle_failure(Box::new(|_ctx, _reason| {
            Box::pin(async move {
                Some(SettleRecoveryResult {
                    recovered: true,
                    result: Some(x402_core::types::SettleResponse {
                        success: true,
                        error_reason: None,
                        error_message: None,
                        payer: Some("0xBuyer".into()),
                        transaction: "0xRecoveredTx".into(),
                        network: "eip155:196".into(),
                        amount: None,
                        status: Some("success".into()),
                        extensions: None,
                    }),
                })
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Settle failed but hook recovered → 200 with PAYMENT-RESPONSE
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Test: onAfterVerify + onAfterSettle — side-effect hooks execute
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_after_hooks_execute() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let after_verify_called = Arc::new(AtomicBool::new(false));
    let after_settle_called = Arc::new(AtomicBool::new(false));

    let av = after_verify_called.clone();
    let as_ = after_settle_called.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_after_verify(Box::new(move |_ctx| {
            let flag = av.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
            })
        }))
        .on_after_settle(Box::new(move |_ctx| {
            let flag = as_.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let payload = test_payment_payload();
    let encoded = encode_payment_signature_header(&payload).unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/weather")
                .header("payment-signature", &encoded)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        after_verify_called.load(Ordering::SeqCst),
        "onAfterVerify should have been called"
    );
    assert!(
        after_settle_called.load(Ordering::SeqCst),
        "onAfterSettle should have been called"
    );
}

// ===========================================================================
// Settle response scenario tests (SDK settlement handling logic)
// ===========================================================================

/// Helper: mount /supported + /verify (always valid) + custom /settle response.
async fn mount_mocks_with_settle(mock_server: &MockServer, settle_json: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kinds": [
                { "x402Version": 2, "scheme": "exact", "network": "eip155:196" },
                { "x402Version": 2, "scheme": "aggr_deferred", "network": "eip155:196" }
            ],
            "extensions": [],
            "signers": { "eip155:*": ["0xFacilitator"] }
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "isValid": true,
            "payer": "0xBuyer"
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(settle_json))
        .mount(mock_server)
        .await;
}

/// Helper: build routes with custom sync_settle setting.
fn test_routes_with_sync_settle(sync_settle: Option<bool>) -> HashMap<String, RoutePaymentConfig> {
    HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "exact".into(),
                price: "$0.001".into(),
                network: "eip155:196".into(),
                pay_to: "0xSeller".into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "Weather data".into(),
            mime_type: "application/json".into(),
            sync_settle,
        },
    )])
}

/// Helper: build routes for aggr_deferred scheme.
fn test_routes_aggr_deferred() -> HashMap<String, RoutePaymentConfig> {
    HashMap::from([(
        "GET /weather".to_string(),
        RoutePaymentConfig {
            accepts: vec![AcceptConfig {
                scheme: "aggr_deferred".into(),
                price: "$0.001".into(),
                network: "eip155:196".into(),
                pay_to: "0xSeller".into(),
                max_timeout_seconds: None,
                extra: None,
            }],
            description: "Weather data".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
        },
    )])
}

/// Helper: build aggr_deferred PaymentPayload.
fn test_payment_payload_deferred() -> PaymentPayload {
    let mut p = test_payment_payload();
    p.accepted.scheme = "aggr_deferred".into();
    p
}

/// Helper: send a payment request and return the response.
async fn send_payment_request(app: Router, payload: &PaymentPayload) -> axum::http::Response<Body> {
    let encoded = encode_payment_signature_header(payload).unwrap();
    app.oneshot(
        Request::builder()
            .uri("/weather")
            .header("payment-signature", &encoded)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

// ---------------------------------------------------------------------------
// Scenario 1: exact + syncSettle=false + success=true → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_async_settle_success() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "pending"
        }),
    )
    .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(None), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Scenario 2: exact + syncSettle=false + success=false → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_async_settle_failure() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": false,
            "payer": "0xBuyer",
            "transaction": "",
            "network": "eip155:196",
            "errorReason": "insufficient_funds"
        }),
    )
    .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(None), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Scenario 3: exact + syncSettle=true + success=true + status="success" → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_settle_success() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "success"
        }),
    )
    .await;

    let server = build_initialized_server(&mock_server).await;
    let layer =
        PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Scenario 4a: exact + syncSettle=true + status="timeout" → poll → status="success" → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_timeout_poll_success() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "timeout"
        }),
    )
    .await;

    // /settle/status returns success on first poll
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/settle/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "success"
        })))
        .mount(&mock_server)
        .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server)
        .poll_deadline(Duration::from_secs(3))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Scenario 4b: exact + syncSettle=true + status="timeout" → poll timeout → hook confirmed → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_timeout_poll_timeout_hook_confirmed() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "timeout"
        }),
    )
    .await;

    // /settle/status always returns pending (will timeout)
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/settle/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "pending"
        })))
        .mount(&mock_server)
        .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server)
        .poll_deadline(Duration::from_secs(2))
        .on_settlement_timeout(Box::new(|_tx, _network| {
            Box::pin(async move { SettlementTimeoutResult { confirmed: true } })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Scenario 4b-alt: poll timeout → hook not confirmed → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_timeout_poll_timeout_hook_not_confirmed() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "timeout"
        }),
    )
    .await;

    // /settle/status always returns pending
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/settle/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "pending"
        })))
        .mount(&mock_server)
        .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server)
        .poll_deadline(Duration::from_secs(2))
        .on_settlement_timeout(Box::new(|_tx, _network| {
            Box::pin(async move { SettlementTimeoutResult { confirmed: false } })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Scenario 4b-no-hook: poll timeout → no hook → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_timeout_poll_timeout_no_hook() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "timeout"
        }),
    )
    .await;

    // /settle/status always returns pending
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/settle/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "pending"
        })))
        .mount(&mock_server)
        .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server)
        .poll_deadline(Duration::from_secs(2))
        // No timeout hook configured
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Scenario 4c: poll settle/status returns success=false → stop polling → hook → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_timeout_poll_failed_hook_confirmed() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "timeout"
        }),
    )
    .await;

    // /settle/status returns failed immediately
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/settle/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": false,
            "transaction": "0xTxHash",
            "network": "eip155:196",
            "status": "failed"
        })))
        .mount(&mock_server)
        .await;

    let server = build_initialized_server(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server)
        .poll_deadline(Duration::from_secs(3))
        .on_settlement_timeout(Box::new(|_tx, _network| {
            Box::pin(async move { SettlementTimeoutResult { confirmed: true } })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    // Poll returned Failed → timeout hook called → confirmed → 200
    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Scenario 5: exact + syncSettle=true + success=false → 402 (no polling)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_exact_sync_settle_failure() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": false,
            "payer": "0xBuyer",
            "transaction": "",
            "network": "eip155:196",
            "errorReason": "insufficient_funds"
        }),
    )
    .await;

    let server = build_initialized_server(&mock_server).await;
    let layer =
        PaymentMiddlewareBuilder::new(test_routes_with_sync_settle(Some(true)), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Scenario 6: aggr_deferred + success=true → 200
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_aggr_deferred_settle_success() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": true,
            "payer": "0xBuyer",
            "transaction": "",
            "network": "eip155:196",
            "status": "success"
        }),
    )
    .await;

    let server = build_initialized_server_with_deferred(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_aggr_deferred(), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload_deferred()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().contains_key("payment-response"));
}

// ---------------------------------------------------------------------------
// Scenario 7: aggr_deferred + success=false → 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_aggr_deferred_settle_failure() {
    let mock_server = MockServer::start().await;
    mount_mocks_with_settle(
        &mock_server,
        json!({
            "success": false,
            "payer": "0xBuyer",
            "transaction": "",
            "network": "eip155:196",
            "errorReason": "rejected"
        }),
    )
    .await;

    let server = build_initialized_server_with_deferred(&mock_server).await;
    let layer = PaymentMiddlewareBuilder::new(test_routes_aggr_deferred(), server).build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload_deferred()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ===========================================================================
// Additional tests: hook combinations, edge cases, context correctness
// ===========================================================================

// ---------------------------------------------------------------------------
// Multiple hooks coexist without interference
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multiple_hooks_coexist() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let call_count = Arc::new(AtomicU32::new(0));
    let c1 = call_count.clone();
    let c2 = call_count.clone();
    let c3 = call_count.clone();
    let c4 = call_count.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_protected_request(Box::new(move |_ctx| {
            let c = c1.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                ProtectedRequestResult {
                    grant_access: false,
                    abort: false,
                    reason: None,
                }
            })
        }))
        .on_before_verify(Box::new(move |_ctx| {
            let c = c2.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                BeforeHookResult {
                    abort: false,
                    reason: None,
                }
            })
        }))
        .on_after_verify(Box::new(move |_ctx| {
            let c = c3.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
            })
        }))
        .on_after_settle(Box::new(move |_ctx| {
            let c = c4.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // All 4 hooks should have been called exactly once
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        4,
        "all 4 hooks should fire"
    );
}

// ---------------------------------------------------------------------------
// onProtectedRequest does not affect free routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_protected_request_hook_not_called_for_free_route() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mock_server = MockServer::start().await;
    let server = build_initialized_server(&mock_server).await;

    let hook_called = Arc::new(AtomicBool::new(false));
    let hc = hook_called.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_protected_request(Box::new(move |_ctx| {
            let flag = hc.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
                ProtectedRequestResult {
                    grant_access: false,
                    abort: true,
                    reason: Some("should not reach".into()),
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .route("/free", get(|| async { Json(json!({"status": "ok"})) }))
        .layer(layer);

    // /free is not in routes config → hook should NOT be called
    let resp = app
        .oneshot(Request::builder().uri("/free").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        !hook_called.load(Ordering::SeqCst),
        "hook should not fire for free routes"
    );
}

// ---------------------------------------------------------------------------
// onVerifyFailure returns None → falls through to 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_verify_failure_hook_returns_none() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, false, false).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_verify_failure(Box::new(|_ctx, _reason| {
            Box::pin(async move { None }) // hook does not recover
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// onVerifyFailure recovered=false → still 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_verify_failure_hook_recovered_false() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, false, false).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_verify_failure(Box::new(|_ctx, _reason| {
            Box::pin(async move {
                Some(VerifyRecoveryResult {
                    recovered: false,
                    result: None,
                })
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// onSettleFailure recovered=false → still 402
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_settle_failure_hook_recovered_false() {
    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, false).await;

    let server = build_initialized_server(&mock_server).await;

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_settle_failure(Box::new(|_ctx, _reason| {
            Box::pin(async move {
                Some(SettleRecoveryResult {
                    recovered: false,
                    result: None,
                })
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
}

// ---------------------------------------------------------------------------
// Hook context contains correct payload and requirements
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_hook_context_contains_correct_data() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let context_valid = Arc::new(AtomicBool::new(false));
    let cv = context_valid.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_before_verify(Box::new(move |ctx| {
            let flag = cv.clone();
            Box::pin(async move {
                // Verify context contains the correct buyer data
                let valid = ctx.payment_payload.accepted.scheme == "exact"
                    && ctx.payment_payload.accepted.network == "eip155:196"
                    && ctx.payment_requirements.scheme == "exact"
                    && ctx.payment_requirements.pay_to == "0xSeller";
                flag.store(valid, Ordering::SeqCst);
                BeforeHookResult {
                    abort: false,
                    reason: None,
                }
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        context_valid.load(Ordering::SeqCst),
        "hook context should contain correct payload data"
    );
}

// ---------------------------------------------------------------------------
// onAfterSettle context contains correct settle response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_after_settle_context_contains_settle_response() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let context_valid = Arc::new(AtomicBool::new(false));
    let cv = context_valid.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_after_settle(Box::new(move |ctx| {
            let flag = cv.clone();
            Box::pin(async move {
                let valid = ctx.settle_response.success
                    && ctx.settle_response.transaction == "0xTxHash"
                    && ctx.settle_response.network == "eip155:196"
                    && ctx.settle_response.payer == Some("0xBuyer".to_string());
                flag.store(valid, Ordering::SeqCst);
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async { Json(json!({"weather": "sunny"})) }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        context_valid.load(Ordering::SeqCst),
        "onAfterSettle should receive correct settle response"
    );
}

// ---------------------------------------------------------------------------
// Handler returns 500 → skip settle (and settle hooks)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_handler_error_skips_settle_and_hooks() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mock_server = MockServer::start().await;
    mount_facilitator_mocks(&mock_server, true, true).await;

    let server = build_initialized_server(&mock_server).await;

    let settle_hook_called = Arc::new(AtomicBool::new(false));
    let shc = settle_hook_called.clone();

    let layer = PaymentMiddlewareBuilder::new(test_routes(), server)
        .on_after_settle(Box::new(move |_ctx| {
            let flag = shc.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
            })
        }))
        .build();

    let app = Router::new()
        .route(
            "/weather",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "boom"})),
                )
            }),
        )
        .layer(layer);

    let resp = send_payment_request(app, &test_payment_payload()).await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        !settle_hook_called.load(Ordering::SeqCst),
        "settle hook should NOT fire when handler errors"
    );
}
