//! Drop-in Axum handlers for session management endpoints.
//!
//! Provides canned implementations of:
//! - `POST /session/settle` — merchant-driven settle (uses the latest
//!   local voucher; SDK signs SettleAuth locally).
//! - `GET  /session/status` — on-chain channel status query.
//!
//! These handlers wrap
//! [`EvmSessionMethod::settle_with_authorization`] /
//! [`EvmSessionMethod::status`], translating
//! [`SaApiError`] / [`VerificationError`] into HTTP responses:
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
//! Note: `POST /session/voucher` is **not** provided here. After
//! receiving a voucher from your own 402 path, the business layer should
//! call [`EvmSessionMethod::submit_voucher`] directly.
//! `POST /session/close` flows through mpp-rs's
//! [`SessionMethod::verify_session`]; the merchant server feeds a
//! `PaymentCredential` to the framework, so no standalone handler is needed.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::session_method::EvmSessionMethod;

/// Body for `POST /session/settle`.
///
/// Only `channelId` is required — cumulative amount and payer signature
/// come from the SDK's local store (saved during `submit_voucher`). The
/// merchant's own authorization (which channel belongs to which
/// merchant) should be handled outside this handler.
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

/// Convert `SaApiError` to an HTTP response: status code is derived
/// from `to_problem_details` (70010 → 404, 70008 → 410, 70004 → 401, ...);
/// the body uses the `Display` text. Merchants who want an RFC 9457
/// JSON body can call the underlying `EvmSessionMethod` directly and
/// build their own response.
fn error_response(err: crate::error::SaApiError) -> Response {
    let problem = err.to_problem_details(None);
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, err.to_string()).into_response()
}

/// Axum handler: `POST /session/settle`.
///
/// Body: `{"channelId": "0x..."}`. On success returns SA API's
/// [`SessionReceipt`] with HTTP 200. On failure, [`SaApiError`] maps to
/// the RFC 9457 ProblemDetails HTTP status; the body is the error
/// message text. Merchants who want a problem+json body can call
/// [`EvmSessionMethod::settle_with_authorization`] directly.
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

    /// 32-byte channel_id (hex with 0x); fixed value for predictable asserts.
    const CHANNEL_ID: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";
    /// 65-byte fake signature — placeholder only; handler tests don't
    /// verify (settle no longer re-checks the voucher; it just reads the store).
    const FAKE_VOUCHER_SIG: &str = "0x";

    /// Build an `EvmSessionMethod` with an injected signer and a preloaded voucher.
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

        let method =
            EvmSessionMethod::with_store(sa as Arc<dyn SaApiClient>, store).with_signer(signer);
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

        // The SDK should have populated payeeSignature / nonce / deadline.
        let captured = sa.last_settle.lock().unwrap().clone().unwrap();
        assert_eq!(captured.cumulative_amount, "1000");
        assert!(captured.payee_signature.starts_with("0x"));
        assert!(!captured.payee_signature.is_empty());
        assert_ne!(captured.nonce, "");
        assert_ne!(captured.deadline, "");
    }

    #[tokio::test]
    async fn settle_unknown_channel_returns_404_with_70010() {
        let sa = Arc::new(MockSa::default());
        // Note: no ChannelRecord preloaded — store get will miss.
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
        // 70010 maps to RFC 9457 ChannelNotFoundError → HTTP 404 (see error::map).
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
        // The `cumulative_amount` field is no longer returned.
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
