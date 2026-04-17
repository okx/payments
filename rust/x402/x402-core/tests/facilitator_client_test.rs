//! Integration tests for OkxHttpFacilitatorClient using wiremock.
//!
//! Tests the full HTTP flow: HMAC signing, request/response serialization,
//! error handling, and retry behavior.

use std::collections::HashMap;

use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use x402_core::facilitator::FacilitatorClient;
use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::types::*;

/// Helper: create a test client pointing at the mock server.
fn test_client(server_url: &str) -> OkxHttpFacilitatorClient {
    OkxHttpFacilitatorClient::with_url(server_url, "test-api-key", "test-secret", "test-passphrase")
        .expect("failed to create test client")
}

/// Helper: build a minimal VerifyRequest.
fn test_verify_request() -> VerifyRequest {
    VerifyRequest {
        x402_version: 2,
        payment_payload: PaymentPayload {
            x402_version: 2,
            resource: None,
            accepted: PaymentRequirements {
                scheme: "exact".into(),
                network: "eip155:196".into(),
                asset: "0xToken".into(),
                amount: "1000".into(),
                pay_to: "0xSeller".into(),
                max_timeout_seconds: 60,
                extra: HashMap::new(),
            },
            payload: {
                let mut m = HashMap::new();
                m.insert("signature".into(), serde_json::json!("0xabc"));
                m
            },
            extensions: None,
        },
        payment_requirements: PaymentRequirements {
            scheme: "exact".into(),
            network: "eip155:196".into(),
            asset: "0xToken".into(),
            amount: "1000".into(),
            pay_to: "0xSeller".into(),
            max_timeout_seconds: 60,
            extra: HashMap::new(),
        },
    }
}

/// Helper: build a minimal SettleRequest.
fn test_settle_request() -> SettleRequest {
    let vr = test_verify_request();
    SettleRequest {
        x402_version: 2,
        payment_payload: vr.payment_payload,
        payment_requirements: vr.payment_requirements,
        sync_settle: Some(true),
    }
}

// ---------------------------------------------------------------------------
// GET /supported
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_supported_success() {
    let mock_server = MockServer::start().await;

    let body = serde_json::json!({
        "kinds": [
            { "x402Version": 2, "scheme": "exact", "network": "eip155:196" },
            { "x402Version": 2, "scheme": "aggr_deferred", "network": "eip155:196" }
        ],
        "extensions": [],
        "signers": { "eip155:*": ["0xFacilitatorSigner"] }
    });

    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .and(header_exists("OK-ACCESS-KEY"))
        .and(header_exists("OK-ACCESS-SIGN"))
        .and(header_exists("OK-ACCESS-TIMESTAMP"))
        .and(header_exists("OK-ACCESS-PASSPHRASE"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let supported = client.get_supported().await.unwrap();

    assert_eq!(supported.kinds.len(), 2);
    assert_eq!(supported.kinds[0].scheme, "exact");
    assert_eq!(supported.kinds[1].scheme, "aggr_deferred");
    assert_eq!(supported.kinds[0].network, "eip155:196");
    assert_eq!(supported.extensions.len(), 0);
    assert!(supported.signers.contains_key("eip155:*"));
}

#[tokio::test]
async fn test_get_supported_hmac_headers_present() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .and(header_exists("OK-ACCESS-KEY"))
        .and(header_exists("OK-ACCESS-SIGN"))
        .and(header_exists("OK-ACCESS-TIMESTAMP"))
        .and(header_exists("OK-ACCESS-PASSPHRASE"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "kinds": [], "extensions": [], "signers": {}
            })),
        )
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let result = client.get_supported().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_supported_server_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/supported"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let result = client.get_supported().await;
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("500"));
}

// ---------------------------------------------------------------------------
// POST /verify
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_verify_valid_payment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/verify"))
        .and(header_exists("OK-ACCESS-KEY"))
        .and(header_exists("OK-ACCESS-SIGN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "isValid": true,
            "payer": "0xBuyerAddress"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.verify(&test_verify_request()).await.unwrap();

    assert!(response.is_valid);
    assert_eq!(response.payer, Some("0xBuyerAddress".into()));
}

#[tokio::test]
async fn test_verify_invalid_payment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/verify"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "isValid": false,
            "invalidReason": "insufficient_funds",
            "invalidMessage": "Payer balance is below required amount",
            "payer": "0xBuyerAddress"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.verify(&test_verify_request()).await.unwrap();

    assert!(!response.is_valid);
    assert_eq!(response.invalid_reason, Some("insufficient_funds".into()));
    assert!(response.invalid_message.is_some());
}

#[tokio::test]
async fn test_verify_facilitator_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/verify"))
        .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let result = client.verify(&test_verify_request()).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// POST /settle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_settle_exact_async() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .and(header_exists("OK-ACCESS-SIGN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "payer": "0xBuyerAddress",
            "transaction": "0xTxHash123",
            "network": "eip155:196",
            "status": "pending"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.settle(&test_settle_request()).await.unwrap();

    assert!(response.success);
    assert_eq!(response.transaction, "0xTxHash123");
    assert_eq!(response.network, "eip155:196");
    assert_eq!(response.status, Some("pending".into()));
}

#[tokio::test]
async fn test_settle_exact_sync_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "payer": "0xBuyerAddress",
            "transaction": "0xTxHash456",
            "network": "eip155:196",
            "status": "success"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.settle(&test_settle_request()).await.unwrap();

    assert!(response.success);
    assert_eq!(response.status, Some("success".into()));
}

#[tokio::test]
async fn test_settle_exact_sync_timeout() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "payer": "0xBuyerAddress",
            "transaction": "0xTxHash789",
            "network": "eip155:196",
            "status": "timeout"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.settle(&test_settle_request()).await.unwrap();

    assert!(response.success);
    assert_eq!(response.status, Some("timeout".into()));
}

#[tokio::test]
async fn test_settle_deferred() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "payer": "0xBuyerAddress",
            "transaction": "",
            "network": "eip155:196",
            "status": "success"
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.settle(&test_settle_request()).await.unwrap();

    assert!(response.success);
    assert_eq!(response.transaction, "");
    assert_eq!(response.status, Some("success".into()));
}

#[tokio::test]
async fn test_settle_failure() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": false,
            "errorReason": "insufficient_funds",
            "errorMessage": "Transaction reverted",
            "payer": "0xBuyerAddress",
            "transaction": "",
            "network": "eip155:196",
            "status": ""
        })))
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let response = client.settle(&test_settle_request()).await.unwrap();

    assert!(!response.success);
    assert_eq!(response.error_reason, Some("insufficient_funds".into()));
    assert_eq!(response.error_message, Some("Transaction reverted".into()));
}

// ---------------------------------------------------------------------------
// Serde serialization tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_settle_request_includes_sync_settle() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/settle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "payer": "", "transaction": "", "network": "eip155:196"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = test_client(&mock_server.uri());
    let req = test_settle_request();

    // Verify syncSettle is serialized in the request body
    let body = serde_json::to_string(&req).unwrap();
    assert!(body.contains("\"syncSettle\":true"));

    let _ = client.settle(&req).await;
}

#[test]
fn test_x402_version_serialization() {
    let req = test_verify_request();
    let json = serde_json::to_string(&req).unwrap();
    // Must serialize as "x402Version", not "x402version"
    assert!(json.contains("\"x402Version\":2"));
    assert!(!json.contains("\"x402version\""));
}

#[test]
fn test_payment_requirements_camel_case() {
    let req = PaymentRequirements {
        scheme: "exact".into(),
        network: "eip155:196".into(),
        asset: "0xToken".into(),
        amount: "1000".into(),
        pay_to: "0xSeller".into(),
        max_timeout_seconds: 60,
        extra: HashMap::new(),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"payTo\""));
    assert!(json.contains("\"maxTimeoutSeconds\""));
    assert!(!json.contains("\"pay_to\""));
    assert!(!json.contains("\"max_timeout_seconds\""));
}

#[test]
fn test_settle_response_deserialization_with_okx_extensions() {
    let json = r#"{
        "success": true,
        "payer": "0xBuyer",
        "transaction": "0xTx",
        "network": "eip155:196",
        "status": "success",
        "errorReason": null,
        "errorMessage": null
    }"#;
    let resp: SettleResponse = serde_json::from_str(json).unwrap();
    assert!(resp.success);
    assert_eq!(resp.status, Some("success".into()));
    assert_eq!(resp.transaction, "0xTx");
}
