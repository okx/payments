//! Drop-in Axum handlers for session management endpoints.
//!
//! Provides canned implementations of:
//! - `POST /session/settle` — 商户主动结算（用本地最新 voucher，SDK 本地签 SettleAuth）
//! - `GET  /session/status` — 链上 channel 状态查询
//!
//! 这些 handler 包装 [`EvmSessionMethod::settle_with_authorization`] /
//! [`EvmSessionMethod::status`]，把 [`SaApiError`] / [`VerificationError`]
//! 翻译成 HTTP 响应：
//!
//! ```ignore
//! use axum::{routing::{post, get}, Router};
//! use mpp_evm::{handlers, EvmSessionMethod};
//! use std::sync::Arc;
//!
//! let session_method: Arc<EvmSessionMethod> = /* build with .with_signer(..) */ todo!();
//! let router: Router = Router::new()
//!     .route("/session/settle", post(handlers::session_settle))
//!     .route("/session/status", get(handlers::session_status))
//!     .with_state(session_method);
//! ```
//!
//! 注：`POST /session/voucher` **不在** SDK handlers 提供 —— 业务层从自己的 402
//! 路径接收 voucher 后调 [`EvmSessionMethod::submit_voucher`]。
//! `POST /session/close` 走 mpp-rs 的 [`SessionMethod::verify_session`]
//! 流程，由商户 server 把 PaymentCredential 喂进框架，无需独立 handler。

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::session_method::EvmSessionMethod;

/// Body for `POST /session/settle`.
///
/// 仅需 channelId — 累计金额、payer 签名都从 SDK 本地 store 取（`submit_voucher`
/// 阶段已存）。商户自己的鉴权（哪个 channel 归哪个商户）应该在 handler 之外做。
#[derive(Debug, Clone, Deserialize)]
pub struct SettleBody {
    #[serde(rename = "channelId")]
    pub channel_id: String,
}

/// Query params for `GET /session/status?channelId=...`.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusQuery {
    #[serde(rename = "channelId")]
    pub channel_id: String,
}

/// 把 `SaApiError` 转成对应的 HTTP 响应:状态码走 `to_problem_details`(70010 → 404,
/// 70008 → 410,70004 → 401,etc.),body 用 `Display` 文本。商户需要 RFC 9457 JSON
/// body 时,可直接调底层 `EvmSessionMethod` 自行组装响应。
fn error_response(err: crate::error::SaApiError) -> Response {
    let problem = err.to_problem_details(None);
    let status = StatusCode::from_u16(problem.status)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, err.to_string()).into_response()
}

/// Axum handler: `POST /session/settle`.
///
/// Body: `{"channelId": "0x..."}`. 成功返回 SA API 的 [`SessionReceipt`] 200。
/// 失败按 [`SaApiError`] 映射到 RFC 9457 ProblemDetails 的 HTTP 状态码,body
/// 是错误 message 文本。商户需要 problem+json 体可直接调
/// [`EvmSessionMethod::settle_with_authorization`]。
///
/// [`SessionReceipt`]: crate::types::SessionReceipt
/// [`SaApiError`]: crate::error::SaApiError
pub async fn session_settle(
    State(method): State<Arc<EvmSessionMethod>>,
    Json(body): Json<SettleBody>,
) -> Response {
    match method.settle_with_authorization(&body.channel_id).await {
        Ok(receipt) => (StatusCode::OK, Json(receipt)).into_response(),
        Err(e) => error_response(e),
    }
}

/// Axum handler: `GET /session/status?channelId=0x...`.
pub async fn session_status(
    State(method): State<Arc<EvmSessionMethod>>,
    Query(q): Query<StatusQuery>,
) -> Response {
    match method.status(&q.channel_id).await {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(e) => error_response(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SaApiError;
    use crate::sa_client::SaApiClient;
    use crate::store::{ChannelRecord, InMemorySessionStore, SessionStore};
    use crate::types::{
        ChannelStatus, ChargeReceipt, CloseRequestPayload, SessionReceipt, SettleRequestPayload,
    };
    use alloy_primitives::{Address, Bytes};
    use alloy_signer_local::PrivateKeySigner;
    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use std::str::FromStr;
    use std::sync::Mutex as StdMutex;
    use tower::ServiceExt;

    #[derive(Default)]
    struct MockSa {
        last_settle: StdMutex<Option<SettleRequestPayload>>,
    }

    #[async_trait]
    impl SaApiClient for MockSa {
        async fn charge_settle(&self, _: &serde_json::Value) -> Result<ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn charge_verify_hash(
            &self,
            _: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_open(&self, _: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(
            &self,
            payload: &SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            *self.last_settle.lock().unwrap() = Some(payload.clone());
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                channel_id: payload.channel_id.clone(),
                reference: Some("0xtx".into()),
                deposit: Some("1000".into()),
                challenge_id: None,
                accepted_cumulative: Some(payload.cumulative_amount.clone()),
                spent: Some(payload.cumulative_amount.clone()),
                confirmations: None,
                units: None,
            })
        }
        async fn session_close(
            &self,
            _: &CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
            Ok(ChannelStatus {
                channel_id: channel_id.into(),
                payer: "0xpayer".into(),
                payee: "0xpayee".into(),
                token: "0xtoken".into(),
                deposit: "10000".into(),
                cumulative_amount: None,
                settled_on_chain: "1000".into(),
                session_status: "OPEN".into(),
                remaining_balance: "9000".into(),
            })
        }
    }

    /// 32 字节 channel_id（hex with 0x），固定值便于断言。
    const CHANNEL_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    /// 65 字节假签名 — 仅占位，handler 测试不验签（settle 不会再校 voucher，只读 store）。
    const FAKE_VOUCHER_SIG: &str = "0x";

    /// 构造一个已注入 signer + 预置 voucher 的 EvmSessionMethod。
    async fn build_method(sa: Arc<MockSa>) -> Arc<EvmSessionMethod> {
        let signer = PrivateKeySigner::random();
        let payee = signer.address();

        let store = Arc::new(InMemorySessionStore::default());
        store
            .put(ChannelRecord {
                channel_id: CHANNEL_ID.into(),
                chain_id: 196,
                escrow_contract: Address::ZERO,
                payer: Address::ZERO,
                payee,
                authorized_signer: Address::ZERO,
                deposit: 10_000,
                highest_voucher_amount: 1_000,
                highest_voucher_signature: Some(Bytes::from_str(FAKE_VOUCHER_SIG).unwrap()),
                min_voucher_delta: None,
                spent: 0,
                units: 0,
            })
            .await;

        let method = EvmSessionMethod::with_store(sa as Arc<dyn SaApiClient>, store)
            .with_signer(signer);
        Arc::new(method)
    }

    fn router(method: Arc<EvmSessionMethod>) -> Router {
        Router::new()
            .route("/session/settle", post(session_settle))
            .route("/session/status", get(session_status))
            .with_state(method)
    }

    #[tokio::test]
    async fn settle_returns_receipt_json_and_signs_payee_authorization() {
        let sa = Arc::new(MockSa::default());
        let method = build_method(sa.clone()).await;
        let app = router(method);

        let body = format!(r#"{{"channelId":"{CHANNEL_ID}"}}"#);
        let req = Request::builder()
            .method("POST")
            .uri("/session/settle")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let parsed: SessionReceipt = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.channel_id, CHANNEL_ID);
        assert_eq!(parsed.accepted_cumulative.as_deref(), Some("1000"));

        // SDK 应该已经填好 payeeSignature / nonce / deadline
        let captured = sa.last_settle.lock().unwrap().clone().unwrap();
        assert_eq!(captured.cumulative_amount, "1000");
        assert!(captured.payee_signature.starts_with("0x"));
        assert!(!captured.payee_signature.is_empty());
        assert_ne!(captured.nonce, "");
        assert_ne!(captured.deadline, "");
    }

    #[tokio::test]
    async fn settle_unknown_channel_returns_500_with_70010() {
        let sa = Arc::new(MockSa::default());
        // 注：不预置 ChannelRecord，store get 会 miss
        let signer = PrivateKeySigner::random();
        let method = EvmSessionMethod::new(sa).with_signer(signer);
        let app = router(Arc::new(method));

        let req = Request::builder()
            .method("POST")
            .uri("/session/settle")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"channelId":"{CHANNEL_ID}"}}"#)))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("70010") || s.to_lowercase().contains("not found"));
    }

    #[tokio::test]
    async fn status_returns_channel_state() {
        let sa = Arc::new(MockSa::default());
        let method = build_method(sa).await;
        let app = router(method);

        let req = Request::builder()
            .method("GET")
            .uri(format!("/session/status?channelId={CHANNEL_ID}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let parsed: ChannelStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.channel_id, CHANNEL_ID);
        assert_eq!(parsed.session_status, "OPEN");
        // cumulative_amount 字段不再返回
        assert!(parsed.cumulative_amount.is_none());
    }

    #[tokio::test]
    async fn status_missing_query_is_400() {
        let sa = Arc::new(MockSa::default());
        let method = build_method(sa).await;
        let app = router(method);

        let req = Request::builder()
            .method("GET")
            .uri("/session/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
