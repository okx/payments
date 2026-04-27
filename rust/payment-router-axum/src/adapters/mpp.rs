//! `MppAdapter` — MPP protocol adapter (spec §3 / §6.2).
//!
//! Thin shell. Delegates verify/HMAC/expiry/SA-API to upstream
//! `ChargeChallenger::verify_payment`; formats headers via upstream
//! `format_www_authenticate` / `format_receipt` pub helpers. No custom
//! crypto, no custom serialization.
//!
//! On request:
//! 1. Parse `Authorization: Payment <b64>` header.
//! 2. Call `challenger.verify_payment(header)` — HMAC + expiry + SA verify
//!    happen inside upstream `Mpp::verify_credential`.
//! 3. On success: call `inner` (real axum handler), then append
//!    `Payment-Receipt` header formatted via upstream `format_receipt`.
//! 4. On failure / missing: return 402 with `WWW-Authenticate: Payment ...`
//!    built from upstream `format_www_authenticate`.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use http::{HeaderMap, HeaderValue, Request, Response, StatusCode, header, request::Parts};
use mpp::protocol::core::headers::{
    PAYMENT_RECEIPT_HEADER, WWW_AUTHENTICATE_HEADER, format_receipt, format_www_authenticate,
};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger};
use serde_json::Value;
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

use crate::adapter::{ChallengeFuture, InnerService, ProtocolAdapter};

/// MPP adapter. Spec §9: built-in priority = 10 (tried before x402).
#[derive(Clone)]
pub struct MppAdapter {
    challenger: Arc<dyn ChargeChallenger>,
    priority: u32,
}

impl MppAdapter {
    /// Construct with the default priority (10).
    pub fn new(challenger: Arc<dyn ChargeChallenger>) -> Self {
        Self {
            challenger,
            priority: 10,
        }
    }

    /// Override the default priority. User extensions should prefer 100+.
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

impl ProtocolAdapter for MppAdapter {
    fn name(&self) -> &str {
        "mpp"
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn detect(&self, parts: &Parts) -> bool {
        parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                // Case-insensitive per RFC 7235. "Payment " prefix is the
                // scheme; trailing content is the credential.
                v.split(',')
                    .map(str::trim)
                    .any(|s| s.len() >= 8 && s[..8].eq_ignore_ascii_case("payment "))
            })
            .unwrap_or(false)
    }

    fn get_challenge<'a>(
        &'a self,
        _parts: &'a Parts,
        route_cfg: &'a Value,
    ) -> ChallengeFuture<'a> {
        let challenger = self.challenger.clone();
        let route_cfg = route_cfg.clone();
        Box::pin(async move {
            let amount = route_cfg
                .get("amount")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "mpp route config missing `amount`".to_string())?;
            let description = route_cfg
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| {
                    // Upstream `ChallengeOptions::description` is `&'static str`.
                    // Leak a copy so we don't have to plumb lifetimes through
                    // every call site. Router configs are built once at
                    // startup; leak is one-shot per route definition.
                    &*Box::leak(s.to_string().into_boxed_str())
                });
            let options = ChallengeOptions { description };
            let challenge = challenger
                .challenge(amount, options)
                .map_err(|e| format!("mpp challenge failed: {e}"))?;
            let www = format_www_authenticate(&challenge)
                .map_err(|e| format!("mpp format_www_authenticate failed: {e}"))?;
            let mut map = HeaderMap::new();
            map.append(
                WWW_AUTHENTICATE_HEADER,
                HeaderValue::from_str(&www).map_err(|e| e.to_string())?,
            );
            Ok(Some(map))
        })
    }

    fn make_service(&self, inner: InnerService) -> InnerService {
        BoxCloneSyncService::new(MppVerifyService {
            inner,
            challenger: self.challenger.clone(),
        })
    }
}

/// Tower Service that runs `challenger.verify_payment` → `inner.call` →
/// injects `Payment-Receipt` header. HMAC/expiry/SA-verify all happen inside
/// the native `ChargeChallenger`.
#[derive(Clone)]
struct MppVerifyService {
    inner: InnerService,
    challenger: Arc<dyn ChargeChallenger>,
}

impl Service<Request<Body>> for MppVerifyService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<
        Box<dyn std::future::Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let challenger = self.challenger.clone();
        let inner = self.inner.clone();

        Box::pin(async move {
            let auth = req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let auth = match auth {
                Some(a) => a,
                None => {
                    // Detect already passed, but header disappeared — produce
                    // a generic 401 (not 402, to avoid leaking challenge
                    // re-prompting semantics that the outer merger owns).
                    return Ok(error_response(StatusCode::UNAUTHORIZED, "missing Authorization header"));
                }
            };

            match challenger.verify_payment(&auth).await {
                Ok(receipt) => {
                    let mut resp = inner.oneshot(req).await?;
                    match format_receipt(&receipt) {
                        Ok(header_str) => match HeaderValue::from_str(&header_str) {
                            Ok(hv) => {
                                // append not insert: user handlers (using
                                // upstream `WithReceipt<T>`) may have already
                                // inserted their own. Duplicate receipts are
                                // cheap; missing receipt is worse.
                                if !resp.headers().contains_key(PAYMENT_RECEIPT_HEADER) {
                                    resp.headers_mut().insert(PAYMENT_RECEIPT_HEADER, hv);
                                }
                            }
                            Err(e) => tracing::error!(err=%e, "invalid Payment-Receipt header"),
                        },
                        Err(e) => tracing::error!(err=%e, "format_receipt failed"),
                    }
                    Ok(resp)
                }
                Err(msg) => {
                    // Credential invalid / expired / HMAC bad. Surface as 402
                    // so the client can retry with a fresh credential.
                    Ok(error_response(StatusCode::PAYMENT_REQUIRED, &msg))
                }
            }
        })
    }
}

fn error_response(status: StatusCode, msg: &str) -> Response<Body> {
    let body = serde_json::json!({
        "type": "about:blank",
        "title": status.canonical_reason().unwrap_or(""),
        "status": status.as_u16(),
        "detail": msg,
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/problem+json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .expect("static error response")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    fn parts_with_header(name: &str, val: &str) -> Parts {
        let req = Request::builder()
            .header(name, val)
            .body(Body::empty())
            .unwrap();
        let (parts, _) = req.into_parts();
        parts
    }

    fn parts_no_auth() -> Parts {
        let (parts, _) = Request::new(Body::empty()).into_parts();
        parts
    }

    // Minimal stub challenger for detect-only tests. get_challenge uses the
    // real challenger in integration tests.
    struct StubChallenger;
    impl ChargeChallenger for StubChallenger {
        fn challenge(
            &self,
            _amount: &str,
            _options: ChallengeOptions,
        ) -> Result<mpp::protocol::core::PaymentChallenge, String> {
            Err("not implemented for this test".into())
        }
        fn verify_payment(
            &self,
            _: &str,
        ) -> Pin<
            Box<dyn std::future::Future<Output = Result<mpp::protocol::core::Receipt, String>> + Send>,
        > {
            Box::pin(async { Err("not implemented".into()) })
        }
    }

    fn adapter() -> MppAdapter {
        MppAdapter::new(Arc::new(StubChallenger))
    }

    #[test]
    fn detect_true_on_payment_scheme() {
        assert!(adapter().detect(&parts_with_header("authorization", "Payment abc123")));
    }

    #[test]
    fn detect_case_insensitive() {
        assert!(adapter().detect(&parts_with_header("authorization", "payment ABC")));
        assert!(adapter().detect(&parts_with_header("authorization", "PAYMENT xyz")));
    }

    #[test]
    fn detect_mixed_schemes() {
        // RFC 9110: comma-separated schemes are allowed.
        assert!(adapter().detect(&parts_with_header(
            "authorization",
            "Bearer tok, Payment abc"
        )));
    }

    #[test]
    fn detect_false_on_bearer_only() {
        assert!(!adapter().detect(&parts_with_header("authorization", "Bearer token")));
    }

    #[test]
    fn detect_false_on_missing() {
        assert!(!adapter().detect(&parts_no_auth()));
    }

    #[test]
    fn name_and_priority() {
        let a = adapter();
        assert_eq!(a.name(), "mpp");
        assert_eq!(a.priority(), 10);
    }
}
