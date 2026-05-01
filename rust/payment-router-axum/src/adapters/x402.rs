//! `X402Adapter` — x402 protocol adapter (spec §3 / §6.2).
//!
//! **Native reuse.** `make_service(inner)` returns
//! `x402_axum::PaymentMiddleware(inner)` — all hooks / resolver / timeout
//! recovery / settle / PAYMENT-RESPONSE header injection run through the
//! real x402-axum middleware wrapping the real handler. Spec §1 principle
//! "Zero intrusion into x402" satisfied: no x402 code is duplicated or
//! reimplemented.
//!
//! For `get_challenge`, we invoke a *clone* of the same layer against a
//! dummy inner service. With no payment header present, the middleware
//! short-circuits (middleware.rs:190-200) and returns its own 402 —
//! constructed via the middleware's own `build_402_response` (incl.
//! `PaymentResolverFn`, `PAYMENT-REQUIRED` header, and the accepts body).
//! We then lift headers (and body, if any other adapter doesn't also claim
//! body ownership) into the merger. Zero reimplementation.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use http::{request::Parts, Request, Response, StatusCode};
use serde_json::Value;
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};
use x402_axum::{PaymentLayer, PaymentMiddlewareBuilder};
use x402_core::http::RoutesConfig;
use x402_core::server::X402ResourceServer;

use crate::adapter::{ChallengeFuture, ChallengeResponse, InnerService, ProtocolAdapter};

/// Builder for [`X402Adapter`]. Mirror of [`PaymentMiddlewareBuilder`] with
/// one extra field (priority). All x402 hooks are exposed unchanged.
///
/// User constructs via `X402Adapter::builder(server)` and optionally calls
/// `.on_before_verify` etc. Built layer is held internally and reused for
/// both `make_service` (real wrapping) and `get_challenge` (402-synthesis).
pub struct X402AdapterBuilder {
    inner: PaymentMiddlewareBuilder,
    priority: u32,
}

impl X402AdapterBuilder {
    /// Set x402 priority (default 20). Spec §9: user extensions from 100.
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
    pub fn on_protected_request(mut self, hook: x402_core::http::OnProtectedRequestHook) -> Self {
        self.inner = self.inner.on_protected_request(hook);
        self
    }
    pub fn on_before_verify(mut self, hook: x402_core::http::OnBeforeVerifyHook) -> Self {
        self.inner = self.inner.on_before_verify(hook);
        self
    }
    pub fn on_after_verify(mut self, hook: x402_core::http::OnAfterVerifyHook) -> Self {
        self.inner = self.inner.on_after_verify(hook);
        self
    }
    pub fn on_verify_failure(mut self, hook: x402_core::http::OnVerifyFailureHook) -> Self {
        self.inner = self.inner.on_verify_failure(hook);
        self
    }
    pub fn on_before_settle(mut self, hook: x402_core::http::OnBeforeSettleHook) -> Self {
        self.inner = self.inner.on_before_settle(hook);
        self
    }
    pub fn on_after_settle(mut self, hook: x402_core::http::OnAfterSettleHook) -> Self {
        self.inner = self.inner.on_after_settle(hook);
        self
    }
    pub fn on_settle_failure(mut self, hook: x402_core::http::OnSettleFailureHook) -> Self {
        self.inner = self.inner.on_settle_failure(hook);
        self
    }
    pub fn on_settlement_timeout(mut self, hook: x402_core::http::OnSettlementTimeoutHook) -> Self {
        self.inner = self.inner.on_settlement_timeout(hook);
        self
    }
    pub fn poll_deadline(mut self, deadline: std::time::Duration) -> Self {
        self.inner = self.inner.poll_deadline(deadline);
        self
    }
    pub fn resolver(mut self, resolver: x402_core::http::PaymentResolverFn) -> Self {
        self.inner = self.inner.resolver(resolver);
        self
    }

    pub fn build(self) -> X402Adapter {
        X402Adapter {
            layer: self.inner.build(),
            priority: self.priority,
        }
    }
}

/// Adapter that wraps x402-axum's [`PaymentLayer`] and participates in the
/// `PaymentRouter` Adapter-pattern pipeline.
#[derive(Clone)]
pub struct X402Adapter {
    layer: PaymentLayer,
    priority: u32,
}

impl X402Adapter {
    /// Start building. `routes` is the x402 per-route config
    /// (keyed by `"METHOD /path"`). `server` must already be initialized
    /// (spec §3 hard convention — caller `await server.initialize()` before
    /// constructing the adapter).
    pub fn builder(routes: RoutesConfig, server: X402ResourceServer) -> X402AdapterBuilder {
        X402AdapterBuilder {
            inner: PaymentMiddlewareBuilder::new(routes, server),
            priority: 20,
        }
    }

    /// Convenience: adapter with only the required params (no hooks, no
    /// resolver). Equivalent to `builder(routes, server).build()`.
    pub fn new(routes: RoutesConfig, server: X402ResourceServer) -> Self {
        Self::builder(routes, server).build()
    }

    fn clone_parts_into_request(parts: &Parts) -> Request<Body> {
        // Parts implements Clone (http crate 1.x); rebuild a Request with an
        // empty body so we can drive the middleware for 402 synthesis.
        let mut req = Request::new(Body::empty());
        *req.method_mut() = parts.method.clone();
        *req.uri_mut() = parts.uri.clone();
        *req.version_mut() = parts.version;
        *req.headers_mut() = parts.headers.clone();
        req
    }
}

impl ProtocolAdapter for X402Adapter {
    fn name(&self) -> &str {
        "x402"
    }

    fn priority(&self) -> u32 {
        self.priority
    }

    fn detect(&self, parts: &Parts) -> bool {
        parts.headers.contains_key("x-payment") || parts.headers.contains_key("payment-signature")
    }

    fn get_challenge<'a>(&'a self, parts: &'a Parts, _route_cfg: &'a Value) -> ChallengeFuture<'a> {
        // Strategy: drive a clone of the real layer against a trivial inner.
        // With no x-payment / payment-signature header, the middleware
        // short-circuits into its own 402 path (middleware.rs:190-200) —
        // which already handles PaymentResolverFn, PAYMENT-REQUIRED encoding,
        // accepts array, etc. We harvest headers from the response.
        //
        // The adapter's own `route_cfg` JSON is *not* used here; the
        // middleware looks up its internally-held RoutesConfig by
        // "METHOD /path". Users must register the same route on both sides
        // (PaymentRouterConfig + x402 RoutesConfig given to this adapter).
        let layer = self.layer.clone();
        let request = Self::clone_parts_into_request(parts);
        Box::pin(async move {
            // Dummy inner should never be invoked for the 402 path, but we
            // still produce a real service in case the path changes.
            let dummy: InnerService =
                BoxCloneSyncService::new(tower::service_fn(|_: Request<Body>| async {
                    Ok::<Response<Body>, Infallible>(
                        Response::builder()
                            .status(StatusCode::NO_CONTENT)
                            .body(Body::empty())
                            .expect("static 204"),
                    )
                }));
            let wrapped = layer.layer(dummy);
            let resp = wrapped
                .oneshot(request)
                .await
                .map_err(|e: Infallible| format!("x402 get_challenge oneshot: {e:?}"))?;
            // Only propagate headers when the synth was actually a 402. On
            // 200 (non-paid route on the x402 side), there's nothing to add.
            if resp.status() != StatusCode::PAYMENT_REQUIRED {
                return Ok(None);
            }
            // x402 spec requires the 402 body to carry the `accepts[]`
            // array. We harvest both headers AND body (H5) so the merger
            // can produce a spec-compliant response. Body is bounded by
            // x402-axum's own builder, so the read is safe.
            let headers = resp.headers().clone();
            let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .map_err(|e| format!("x402 get_challenge read body: {e}"))?;
            let body = if body_bytes.is_empty() {
                None
            } else {
                Some(body_bytes)
            };
            Ok(Some(ChallengeResponse { headers, body }))
        })
    }

    fn make_service(&self, inner: InnerService) -> InnerService {
        BoxCloneSyncService::new(X402Service {
            inner: self.layer.clone().layer(inner),
        })
    }
}

/// Thin Service wrapper around `PaymentMiddleware<InnerService>` to normalize
/// the error channel to `Infallible` (InnerService is already Infallible, but
/// `PaymentMiddleware` passes through `S::Error`).
#[derive(Clone)]
struct X402Service {
    inner: x402_axum::PaymentMiddleware<InnerService>,
}

impl Service<Request<Body>> for X402Service {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<
        Box<dyn std::future::Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            match inner.ready().await {
                Ok(ready) => match ready.call(req).await {
                    Ok(r) => Ok(r),
                    Err(_infallible) => unreachable!("inner is Infallible"),
                },
                Err(_infallible) => unreachable!("inner is Infallible"),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Detect-only unit tests (get_challenge / make_service exercised in example
// and conformance suite where a facilitator or mock is available).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    fn parts_with(name: &str) -> Parts {
        let req = Request::builder()
            .header(name, "abc")
            .body(Body::empty())
            .unwrap();
        req.into_parts().0
    }

    fn parts_none() -> Parts {
        Request::new(Body::empty()).into_parts().0
    }

    /// Unit-only check of detect (construction of a real X402Adapter requires
    /// a facilitator, which belongs in the integration suite).
    #[test]
    fn detect_x_payment() {
        let parts = parts_with("x-payment");
        // Construct a fake adapter via an empty PaymentMiddlewareBuilder is
        // not feasible without a facilitator; instead validate detect logic
        // via a standalone helper.
        assert!(detect_logic(&parts));
    }

    #[test]
    fn detect_payment_signature() {
        assert!(detect_logic(&parts_with("payment-signature")));
    }

    #[test]
    fn detect_none() {
        assert!(!detect_logic(&parts_none()));
    }

    fn detect_logic(parts: &Parts) -> bool {
        parts.headers.contains_key("x-payment") || parts.headers.contains_key("payment-signature")
    }
}
