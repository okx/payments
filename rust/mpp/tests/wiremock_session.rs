//! Wiremock-driven integration tests for session endpoints.
//!
//! Verifies wire-format alignment between [`OkxSaApiClient`] and the SA API
//! `/api/v6/pay/mpp/session/*` paths:
//! - `/session/settle` — POST 扁平 payload(含 voucherSig/payeeSig/nonce/deadline)
//! - `/session/close`  — POST 扁平 payload(同上字段)
//! - `/session/status` — GET ?channelId=... → ChannelStatus(无 cumulativeAmount 字段)
//!
//! 这些测试验证 SDK → SA API 的 HTTP 契约：路径、方法、body 字段名（camelCase）、
//! 响应反序列化形状、SaApiError 映射。

use mpp_evm::{
    CloseRequestPayload, OkxSaApiClient, SaApiClient, SettleRequestPayload,
};
use serde_json::Value;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 建一个连到 mock server 的客户端。
fn client_for(server: &MockServer) -> OkxSaApiClient {
    OkxSaApiClient::with_base_url(
        server.uri(),
        "test-key".into(),
        "test-secret".into(),
        "test-passphrase".into(),
    )
}

/// 标准 SA API 包装：`{ code: 0, data: {...}, msg: "" }`。
fn sa_ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "code": 0,
        "data": data,
        "msg": "",
    }))
}

fn sa_err(code: u32, msg: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({
        "code": code,
        "data": serde_json::Value::Null,
        "msg": msg,
    }))
}

const CHANNEL_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

fn settle_payload() -> SettleRequestPayload {
    SettleRequestPayload {
        action: Some("settle".into()),
        channel_id: CHANNEL_ID.into(),
        cumulative_amount: "250000".into(),
        voucher_signature:
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"
                .into(),
        payee_signature:
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb1b"
                .into(),
        nonce: "1789032400000000000000000000000000001".into(),
        deadline: "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            .into(),
    }
}

fn close_payload(voucher_sig: &str) -> CloseRequestPayload {
    CloseRequestPayload {
        action: Some("close".into()),
        channel_id: CHANNEL_ID.into(),
        cumulative_amount: "1000000".into(),
        voucher_signature: voucher_sig.into(),
        payee_signature:
            "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc1c"
                .into(),
        nonce: "9".into(),
        deadline: "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            .into(),
    }
}

#[tokio::test]
async fn settle_posts_camel_case_body_with_all_draft2_fields() {
    let server = MockServer::start().await;
    let payload = settle_payload();
    // Spec: body 外层 wrap 一个 payload 字段
    let expected_body: Value =
        serde_json::json!({ "payload": serde_json::to_value(&payload).unwrap() });

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/settle"))
        .and(header("OK-ACCESS-KEY", "test-key"))
        .and(header("Content-Type", "application/json"))
        .and(body_json(&expected_body))
        .respond_with(sa_ok(serde_json::json!({
            "method": "evm",
            "intent": "session",
            "status": "success",
            "timestamp": "2026-04-22T00:00:00Z",
            "chainId": 196,
            "channelId": CHANNEL_ID,
            "reference": "0xtxhash",
            "deposit": "1000000",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let receipt = client.session_settle(&payload).await.unwrap();
    assert_eq!(receipt.channel_id, CHANNEL_ID);
    assert_eq!(receipt.deposit.as_deref(), Some("1000000"));
}

#[tokio::test]
async fn close_with_voucher_sig_posts_payload() {
    let server = MockServer::start().await;
    let payload = close_payload(
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c",
    );
    let expected_body: Value =
        serde_json::json!({ "payload": serde_json::to_value(&payload).unwrap() });

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/close"))
        .and(body_json(&expected_body))
        .respond_with(sa_ok(serde_json::json!({
            "method": "evm",
            "intent": "session",
            "status": "success",
            "timestamp": "2026-04-22T00:00:00Z",
            "chainId": 196,
            "channelId": CHANNEL_ID,
            "reference": "0xclosetx",
            "deposit": "1000000",
            "spent": "1000000",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let receipt = client.session_close(&payload).await.unwrap();
    assert_eq!(receipt.channel_id, CHANNEL_ID);
    assert_eq!(receipt.spent.as_deref(), Some("1000000"));
}

#[tokio::test]
async fn close_waiver_branch_accepts_empty_voucher_signature() {
    let server = MockServer::start().await;
    // waiver path: payer 没有产生 voucher，cumulativeAmount=0，voucherSignature=""
    let payload = CloseRequestPayload {
        action: Some("close".into()),
        channel_id: CHANNEL_ID.into(),
        cumulative_amount: "0".into(),
        voucher_signature: String::new(),
        payee_signature:
            "0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd1b"
                .into(),
        nonce: "1".into(),
        deadline: "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            .into(),
    };

    // 关键断言：voucherSignature 必须以空串(非 null / 非省略)发送
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/close"))
        .and(body_json(serde_json::json!({
            "payload": {
            "action": "close",
            "channelId": CHANNEL_ID,
            "cumulativeAmount": "0",
            "voucherSignature": "",
            "payeeSignature": payload.payee_signature,
            "nonce": "1",
            "deadline": payload.deadline,
            }
        })))
        .respond_with(sa_ok(serde_json::json!({
            "method": "evm",
            "intent": "session",
            "status": "success",
            "timestamp": "2026-04-22T00:00:00Z",
            "chainId": 196,
            "channelId": CHANNEL_ID,
            "reference": "0xwaivertx",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let receipt = client.session_close(&payload).await.unwrap();
    assert_eq!(receipt.channel_id, CHANNEL_ID);
}

#[tokio::test]
async fn status_get_returns_channel_state_without_cumulative_amount() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v6/pay/mpp/session/status"))
        .and(query_param("channelId", CHANNEL_ID))
        .respond_with(sa_ok(serde_json::json!({
            "channelId": CHANNEL_ID,
            "payer": "0xpayer000000000000000000000000000000000000",
            "payee": "0xpayee000000000000000000000000000000000000",
            "token": "0xtoken000000000000000000000000000000000000",
            "deposit": "1000000",
            "settledOnChain": "0",
            "sessionStatus": "OPEN",
            "remainingBalance": "999900000",
            // cumulativeAmount 不在 status 响应中
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let status = client.session_status(CHANNEL_ID).await.unwrap();
    assert_eq!(status.channel_id, CHANNEL_ID);
    assert_eq!(status.session_status, "OPEN");
    assert!(status.cumulative_amount.is_none());
}

#[tokio::test]
async fn settle_70004_invalid_signature_propagates_as_sa_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/settle"))
        .respond_with(sa_err(70004, "invalid signature"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client.session_settle(&settle_payload()).await.unwrap_err();
    assert_eq!(err.code, 70004);
    assert!(err.msg.contains("invalid"));
}

#[tokio::test]
async fn close_70008_channel_finalized_propagates() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/close"))
        .respond_with(sa_err(70008, "channel finalized"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client
        .session_close(&close_payload("0x"))
        .await
        .unwrap_err();
    assert_eq!(err.code, 70008);
}

#[tokio::test]
async fn session_open_posts_credential_value_unchanged() {
    let server = MockServer::start().await;

    // session_open 仍接 &serde_json::Value（透传 credential，body shape 由
    // payer client 决定，包含 voucherSignature/authorizedSigner/salt 等字段）。
    let credential = serde_json::json!({
        "challenge": { "id": "ch-open-1", "method": "evm" },
        "payload": {
            "type": "transaction",
            "channelId": CHANNEL_ID,
            "salt": "0xfeedface00000000000000000000000000000000000000000000000000000000",
            "voucherSignature": "0x",
            "authorizedSigner": "0x0000000000000000000000000000000000000001",
        }
    });

    Mock::given(method("POST"))
        .and(path("/api/v6/pay/mpp/session/open"))
        .and(body_json(&credential))
        .respond_with(sa_ok(serde_json::json!({
            "method": "evm",
            "intent": "session",
            "status": "success",
            "timestamp": "2026-04-22T00:00:00Z",
            "chainId": 196,
            "channelId": CHANNEL_ID,
            "reference": "0xopentx",
            "deposit": "1000000",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let receipt = client.session_open(&credential).await.unwrap();
    assert_eq!(receipt.channel_id, CHANNEL_ID);
    assert_eq!(receipt.deposit.as_deref(), Some("1000000"));
}
