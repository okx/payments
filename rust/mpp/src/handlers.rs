//! Drop-in Axum handlers for session management endpoints.
//!
//! Provides canned implementations of:
//! - `POST /session/settle` (seller-initiated mid-session settlement)
//! - `GET  /session/status` (read-only channel state lookup)
//!
//! These wrap [`EvmSessionMethod::settle`] and [`EvmSessionMethod::status`] and
//! handle error → HTTP status translation so sellers can mount them directly:
//!
//! ```ignore
//! use axum::{routing::{post, get}, Router};
//! use mpp_evm::{handlers, EvmSessionMethod};
//! use std::sync::Arc;
//!
//! let session_method: Arc<EvmSessionMethod> = /* build */ todo!();
//! let router: Router = Router::new()
//!     .route("/session/settle", post(handlers::session_settle))
//!     .route("/session/status", get(handlers::session_status))
//!     .with_state(session_method);
//! ```

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use crate::session_method::EvmSessionMethod;

/// Body for `POST /session/settle`.
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

/// Axum handler: `POST /session/settle`.
///
/// Body: `{"channelId": "0x..."}`. On success, returns the
/// [`crate::types::SessionReceipt`] from SA API as JSON with HTTP 200. On
/// failure, propagates the SA error message as HTTP 500 (the handler does not
/// currently re-map to RFC 9457 — callers that need problem details should use
/// [`EvmSessionMethod::settle`] directly and format the error themselves).
pub async fn session_settle(
    State(method): State<Arc<EvmSessionMethod>>,
    Json(body): Json<SettleBody>,
) -> Response {
    match method.settle(&body.channel_id).await {
        Ok(receipt) => (StatusCode::OK, Json(receipt)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Axum handler: `GET /session/status?channelId=0x...`.
pub async fn session_status(
    State(method): State<Arc<EvmSessionMethod>>,
    Query(q): Query<StatusQuery>,
) -> Response {
    match method.status(&q.channel_id).await {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SaApiError;
    use crate::sa_client::SaApiClient;
    use crate::types::{ChannelStatus, ChargeReceipt, SessionReceipt};
    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    #[derive(Default)]
    struct MockSa;

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
        async fn session_voucher(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(&self, channel_id: &str) -> Result<SessionReceipt, SaApiError> {
            Ok(SessionReceipt {
                method: "evm".into(),
                intent: "session".into(),
                status: "success".into(),
                timestamp: "2026-04-01T12:00:00Z".into(),
                chain_id: 196,
                challenge_id: "ch-1".into(),
                channel_id: channel_id.into(),
                accepted_cumulative: "1000".into(),
                spent: Some("1000".into()),
                reference: Some("0xtx".into()),
                confirmations: None,
                units: None,
            })
        }
        async fn session_close(&self, _: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_status(&self, channel_id: &str) -> Result<ChannelStatus, SaApiError> {
            Ok(ChannelStatus {
                channel_id: channel_id.into(),
                payer: "0xpayer".into(),
                payee: "0xpayee".into(),
                token: "0xtoken".into(),
                deposit: "10000".into(),
                cumulative_amount: "1000".into(),
                settled_on_chain: "1000".into(),
                session_status: "OPEN".into(),
                remaining_balance: "9000".into(),
            })
        }
    }

    fn router() -> Router {
        let method = Arc::new(EvmSessionMethod::new(Arc::new(MockSa)));
        Router::new()
            .route("/session/settle", post(session_settle))
            .route("/session/status", get(session_status))
            .with_state(method)
    }

    #[tokio::test]
    async fn settle_returns_receipt_json() {
        let app = router();
        let req = Request::builder()
            .method("POST")
            .uri("/session/settle")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"channelId":"0xabc"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let parsed: SessionReceipt = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.channel_id, "0xabc");
        assert_eq!(parsed.spent.as_deref(), Some("1000"));
    }

    #[tokio::test]
    async fn status_returns_channel_state() {
        let app = router();
        let req = Request::builder()
            .method("GET")
            .uri("/session/status?channelId=0xdef")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
        let parsed: ChannelStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.channel_id, "0xdef");
        assert_eq!(parsed.session_status, "OPEN");
    }

    #[tokio::test]
    async fn status_missing_query_is_400() {
        let app = router();
        let req = Request::builder()
            .method("GET")
            .uri("/session/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
