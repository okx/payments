//! Integration tests for the subscription facilitator endpoints (wiremock).

use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use x402_core::http::OkxHttpFacilitatorClient;
use x402_core::subscription::*;

fn test_client(url: &str) -> OkxHttpFacilitatorClient {
    OkxHttpFacilitatorClient::with_url(url, "test-api-key", "test-secret", "test-passphrase")
        .expect("client")
}

fn sample_create_req() -> CreateSubscriptionRequest {
    serde_json::from_value(serde_json::json!({
        "chainIndex": 196,
        "terms": {
            "payer": "0xp", "merchant": "0xm", "facilitator": "0xf", "token": "0xt",
            "amountPerPeriod": "5000000", "periodSec": 2592000, "maxPeriods": 12, "startAt": 0,
            "initialChargePeriods": 1, "initialChargeAmount": "5000000", "termsDeadline": 1750000000u64,
            "permitHash": "0xph", "salt": "0xsalt", "planTier": 2,
            "changeFromSubId": "0x00", "changeEffectiveAt": 0
        },
        "permit": { "details": {"token":"0xt","amount":"60000000","expiration":1782000000u64,"nonce":7}, "spender":"0xsub", "sigDeadline":"1750000000" },
        "termsSig": "0xts",
        "permitSig": "0xps",
        "syncSettle": true
    }))
    .unwrap()
}

#[tokio::test]
async fn create_subscription_posts_signed_and_unwraps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/subscriptions"))
        .and(header_exists("OK-ACCESS-KEY"))
        .and(header_exists("OK-ACCESS-SIGN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0,
            "data": { "subId": "0xsub", "txHash": "0xtx", "state": 1 },
            "msg": ""
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let resp = client.create_subscription(&sample_create_req()).await.unwrap();
    assert_eq!(resp.sub_id, "0xsub");
    assert_eq!(resp.tx_hash.as_deref(), Some("0xtx"));
    assert_eq!(resp.state, 1);
}

#[tokio::test]
async fn charge_posts_to_charge_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/subscriptions/charge"))
        .and(header_exists("OK-ACCESS-KEY"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0,
            "data": { "subId": "0xsub", "period": 2, "txHash": "0xt", "state": 1, "planChangeTriggered": false },
            "msg": ""
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let r = client.charge("0xsub", true).await.unwrap();
    assert_eq!(r.period, 2);
    assert!(!r.plan_change_triggered);
}

#[tokio::test]
async fn get_subscription_reads_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/subscriptions/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0,
            "data": { "subId": "0xsub", "state": 1, "isActive": true, "currentPeriod": 3 },
            "msg": ""
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let s = client.get_subscription("0xsub").await.unwrap();
    assert_eq!(s.state, 1);
    assert!(s.is_active);
    assert_eq!(s.current_period, 3);
}

#[tokio::test]
async fn facilitator_error_status_is_surfaced() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/x402/subscriptions"))
        .respond_with(ResponseTemplate::new(400).set_body_string("permit_hash_mismatch"))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let err = client.create_subscription(&sample_create_req()).await.unwrap_err();
    assert!(format!("{err}").contains("permit_hash_mismatch"));
}

#[tokio::test]
async fn get_pending_change_returns_row() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/subscriptions/pending"))
        .and(header_exists("OK-ACCESS-KEY"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0,
            "data": { "subId": "0xsub", "newSubId": "0xnew", "effectiveFromPeriod": 3, "state": 0 },
            "msg": ""
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    let pending = client.get_pending_change("0xsub").await.unwrap().expect("some");
    assert_eq!(pending.new_sub_id, "0xnew");
    assert_eq!(pending.effective_from_period, 3);
    assert_eq!(pending.state, 0);
}

#[tokio::test]
async fn get_pending_change_none_when_null_data() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/x402/subscriptions/pending"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "code": 0, "data": null, "msg": ""
        })))
        .mount(&server)
        .await;

    let client = test_client(&server.uri());
    assert!(client.get_pending_change("0xsub").await.unwrap().is_none());
}
