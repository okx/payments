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

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::Body;
use http::{header, request::Parts, HeaderMap, HeaderValue, Request, Response, StatusCode};
use mpp::protocol::core::headers::{
    format_receipt, format_www_authenticate, PAYMENT_RECEIPT_HEADER, WWW_AUTHENTICATE_HEADER,
};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger};
use serde_json::Value;
use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceExt};

use crate::adapter::{ChallengeFuture, ChallengeResponse, InnerService, ProtocolAdapter};

/// MPP adapter. Spec §9: built-in priority = 10 (tried before x402).
#[derive(Clone)]
pub struct MppAdapter {
    challenger: Arc<dyn ChargeChallenger>,
    priority: u32,
    /// Lazily-leaked `description` cache. Upstream
    /// `ChallengeOptions::description` is `Option<&'static str>`, so we have
    /// to leak. Caching by source string bounds total leaked memory by the
    /// number of *distinct* descriptions across route configs (typically
    /// proportional to route count) — without it, every unauthorized request
    /// would leak the description anew, giving attackers an OOM DoS vector.
    description_cache: Arc<Mutex<HashMap<String, &'static str>>>,
}

impl MppAdapter {
    /// Construct with the default priority (10).
    pub fn new(challenger: Arc<dyn ChargeChallenger>) -> Self {
        Self {
            challenger,
            priority: 10,
            description_cache: Arc::new(Mutex::new(HashMap::new())),
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

    fn get_challenge<'a>(&'a self, _parts: &'a Parts, route_cfg: &'a Value) -> ChallengeFuture<'a> {
        let challenger = self.challenger.clone();
        let route_cfg = route_cfg.clone();
        let description_cache = self.description_cache.clone();
        Box::pin(async move {
            let amount = route_cfg
                .get("amount")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "mpp route config missing `amount`".to_string())?;
            let description = route_cfg
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| {
                    // Upstream `ChallengeOptions::description` is
                    // `Option<&'static str>`, so a `&'static str` is required
                    // here. Leak the heap copy on first sight and cache it,
                    // so repeated 402s for the same route don't accumulate
                    // leaks (DoS guard). Cache size is bounded by the number
                    // of distinct descriptions ever observed.
                    let mut guard = description_cache.lock().unwrap();
                    *guard
                        .entry(s.to_string())
                        .or_insert_with(|| Box::leak(s.to_string().into_boxed_str()))
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
            // MPP carries everything in headers; let the merger use its
            // default RFC 9457 body (problem+json minimal form).
            Ok(Some(ChallengeResponse::headers_only(map)))
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
                    return Ok(error_response(
                        StatusCode::UNAUTHORIZED,
                        "missing Authorization header",
                    ));
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
            Box<
                dyn std::future::Future<Output = Result<mpp::protocol::core::Receipt, String>>
                    + Send,
            >,
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

    /// Stub challenger that returns a fixed dummy `PaymentChallenge`, just
    /// enough for `get_challenge` to run end-to-end and exercise the
    /// description-leak cache. Real signing is not exercised here.
    struct OkChallenger;
    impl ChargeChallenger for OkChallenger {
        fn challenge(
            &self,
            _amount: &str,
            _options: ChallengeOptions,
        ) -> Result<mpp::protocol::core::PaymentChallenge, String> {
            use mpp::protocol::core::Base64UrlJson;
            let req = Base64UrlJson::from_value(&serde_json::json!({"amount": "1"}))
                .map_err(|e| e.to_string())?;
            Ok(mpp::protocol::core::PaymentChallenge::new(
                "id-1",
                "test-realm",
                "evm",
                "charge",
                req,
            ))
        }
        fn verify_payment(
            &self,
            _: &str,
        ) -> Pin<
            Box<
                dyn std::future::Future<Output = Result<mpp::protocol::core::Receipt, String>>
                    + Send,
            >,
        > {
            Box::pin(async { Err("not implemented".into()) })
        }
    }

    #[tokio::test]
    async fn get_challenge_caches_description_leak_across_calls() {
        // Regression for the per-request `Box::leak` DoS: hitting
        // `get_challenge` N times with the same `description` must leak
        // exactly once (cache size stays at 1 after warm-up).
        let adapter = MppAdapter::new(Arc::new(OkChallenger));
        let cfg = serde_json::json!({
            "amount": "1000",
            "description": "Premium photo access",
        });
        let parts = parts_no_auth();

        for _ in 0..100 {
            let _ = adapter
                .get_challenge(&parts, &cfg)
                .await
                .expect("challenge ok");
        }
        let cache = adapter.description_cache.lock().unwrap();
        assert_eq!(
            cache.len(),
            1,
            "100 identical-description requests must leak exactly one string, got {} cached",
            cache.len()
        );
    }

    #[tokio::test]
    async fn get_challenge_caches_distinct_descriptions_separately() {
        // Distinct descriptions each leak once (bounded by distinct count).
        let adapter = MppAdapter::new(Arc::new(OkChallenger));
        let parts = parts_no_auth();

        for desc in ["one-shot photo", "premium api", "byte-streaming"] {
            let cfg = serde_json::json!({ "amount": "1", "description": desc });
            let _ = adapter
                .get_challenge(&parts, &cfg)
                .await
                .expect("challenge ok");
            let _ = adapter
                .get_challenge(&parts, &cfg)
                .await
                .expect("challenge ok again — cached");
        }
        let cache = adapter.description_cache.lock().unwrap();
        assert_eq!(
            cache.len(),
            3,
            "3 distinct descriptions ⇒ 3 cache entries, got {}",
            cache.len()
        );
    }
}
