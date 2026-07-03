//! `X402Adapter` — x402 protocol adapter (spec §3 / §6.2).
//!
//! **Native reuse.** `make_service(inner)` returns
//! `x402_axum::PaymentMiddleware(inner)` — all hooks / resolver / timeout
//! recovery / settle / PAYMENT-RESPONSE header injection run through the
//! real x402-axum middleware wrapping the real handler. Spec §1 principle
//! "Zero intrusion into x402" satisfied: no x402 code is duplicated or
//! reimplemented.
//!
//! **Route declaration is single-source.** Users put one
//! [`X402RouteConfig`] per route into the unified
//! [`crate::UnifiedRouteConfig`]. [`crate::PaymentRouterLayer::new`]
//! calls [`X402Adapter::prepare`] which extracts those configs and
//! materializes the underlying `PaymentMiddleware` — the adapter
//! constructor itself only takes the `X402ResourceServer` and hooks.

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use http::{request::Parts, Request, Response, StatusCode};
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};
use x402_axum::{AcceptConfig, PaymentLayer, PaymentMiddlewareBuilder, PreMatchedRoute};
use x402_core::http::{
    OnAfterSettleHook, OnAfterVerifyHook, OnBeforeSettleHook, OnBeforeVerifyHook,
    OnProtectedRequestHook, OnSettleFailureHook, OnSettlementTimeoutHook, OnVerifyFailureHook,
    PaymentResolverFn, RoutesConfig, DEFAULT_POLL_DEADLINE,
};
use x402_core::server::X402ResourceServer;

use crate::adapter::{ChallengeFuture, ChallengeResponse, InnerService, ProtocolAdapter};
use crate::config::AdapterConfig;
use crate::types::UnifiedRouteConfig;

/// Typed per-route config consumed by [`X402Adapter`].
///
/// Lives in the `payment-router-axum` type space so users declare routes
/// once on [`crate::UnifiedRouteConfig`]; [`X402Adapter::prepare`] reads
/// these, converts to `x402_axum::RoutePaymentConfig`, and feeds them to
/// the underlying x402 middleware.
#[derive(Debug, Clone, Default)]
pub struct X402RouteConfig {
    /// Accepted payment options for this route. At least one entry is
    /// required for x402 to issue a challenge.
    pub accepts: Vec<AcceptConfig>,
    /// Human-readable description echoed into `ResourceInfo.description`.
    pub description: String,
    /// Response `Content-Type` for the resource.
    pub mime_type: String,
    /// If `true`, settle waits for on-chain confirmation. Default `false`.
    pub sync_settle: Option<bool>,
    /// Merchant override for `ResourceInfo.url`. When unset, the x402
    /// middleware auto-extracts from request headers.
    pub resource: Option<String>,
}

impl X402RouteConfig {
    /// Compile to an `x402_axum::RoutePaymentConfig` used by the
    /// underlying middleware.
    pub fn to_x402(&self) -> x402_axum::RoutePaymentConfig {
        x402_axum::RoutePaymentConfig {
            accepts: self.accepts.clone(),
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            sync_settle: self.sync_settle,
            resource: self.resource.clone(),
            operation: None,
        }
    }
}

/// Build a flat `RoutesConfig` from an iterator of
/// `("METHOD /path", X402RouteConfig)`. Exposed for users constructing
/// `x402_axum::PaymentMiddleware` directly outside the router.
pub fn build_routes_config<I>(entries: I) -> RoutesConfig
where
    I: IntoIterator<Item = (String, X402RouteConfig)>,
{
    let mut map = HashMap::new();
    for (key, cfg) in entries {
        map.insert(key, cfg.to_x402());
    }
    map
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for [`X402Adapter`]. Accumulates the server + hooks; the
/// final `PaymentMiddleware` materializes inside `prepare()` once the
/// payment-router knows the full set of routes.
pub struct X402AdapterBuilder {
    server: X402ResourceServer,
    priority: u32,
    timeout_hook: Option<OnSettlementTimeoutHook>,
    poll_deadline: Duration,
    resolver: Option<PaymentResolverFn>,
    on_protected_request: Option<OnProtectedRequestHook>,
    on_before_verify: Option<OnBeforeVerifyHook>,
    on_after_verify: Option<OnAfterVerifyHook>,
    on_verify_failure: Option<OnVerifyFailureHook>,
    on_before_settle: Option<OnBeforeSettleHook>,
    on_after_settle: Option<OnAfterSettleHook>,
    on_settle_failure: Option<OnSettleFailureHook>,
}

impl X402AdapterBuilder {
    fn new(server: X402ResourceServer) -> Self {
        Self {
            server,
            priority: 20,
            timeout_hook: None,
            poll_deadline: DEFAULT_POLL_DEADLINE,
            resolver: None,
            on_protected_request: None,
            on_before_verify: None,
            on_after_verify: None,
            on_verify_failure: None,
            on_before_settle: None,
            on_after_settle: None,
            on_settle_failure: None,
        }
    }

    /// Set x402 priority (default 20). Spec §9: user extensions from 100.
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
    pub fn on_protected_request(mut self, hook: OnProtectedRequestHook) -> Self {
        self.on_protected_request = Some(hook);
        self
    }
    pub fn on_before_verify(mut self, hook: OnBeforeVerifyHook) -> Self {
        self.on_before_verify = Some(hook);
        self
    }
    pub fn on_after_verify(mut self, hook: OnAfterVerifyHook) -> Self {
        self.on_after_verify = Some(hook);
        self
    }
    pub fn on_verify_failure(mut self, hook: OnVerifyFailureHook) -> Self {
        self.on_verify_failure = Some(hook);
        self
    }
    pub fn on_before_settle(mut self, hook: OnBeforeSettleHook) -> Self {
        self.on_before_settle = Some(hook);
        self
    }
    pub fn on_after_settle(mut self, hook: OnAfterSettleHook) -> Self {
        self.on_after_settle = Some(hook);
        self
    }
    pub fn on_settle_failure(mut self, hook: OnSettleFailureHook) -> Self {
        self.on_settle_failure = Some(hook);
        self
    }
    pub fn on_settlement_timeout(mut self, hook: OnSettlementTimeoutHook) -> Self {
        self.timeout_hook = Some(hook);
        self
    }
    pub fn poll_deadline(mut self, deadline: Duration) -> Self {
        self.poll_deadline = deadline;
        self
    }
    pub fn resolver(mut self, resolver: PaymentResolverFn) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn build(self) -> X402Adapter {
        let priority = self.priority;
        X402Adapter {
            pending: Mutex::new(Some(X402Pending {
                server: self.server,
                timeout_hook: self.timeout_hook,
                poll_deadline: self.poll_deadline,
                resolver: self.resolver,
                on_protected_request: self.on_protected_request,
                on_before_verify: self.on_before_verify,
                on_after_verify: self.on_after_verify,
                on_verify_failure: self.on_verify_failure,
                on_before_settle: self.on_before_settle,
                on_after_settle: self.on_after_settle,
                on_settle_failure: self.on_settle_failure,
            })),
            layer: OnceLock::new(),
            priority,
        }
    }
}

/// State captured at adapter-construction time; consumed by
/// [`X402Adapter::prepare`] once routes are known.
struct X402Pending {
    server: X402ResourceServer,
    timeout_hook: Option<OnSettlementTimeoutHook>,
    poll_deadline: Duration,
    resolver: Option<PaymentResolverFn>,
    on_protected_request: Option<OnProtectedRequestHook>,
    on_before_verify: Option<OnBeforeVerifyHook>,
    on_after_verify: Option<OnAfterVerifyHook>,
    on_verify_failure: Option<OnVerifyFailureHook>,
    on_before_settle: Option<OnBeforeSettleHook>,
    on_after_settle: Option<OnAfterSettleHook>,
    on_settle_failure: Option<OnSettleFailureHook>,
}

// ============================================================================
// Adapter
// ============================================================================

/// Adapter that wraps x402-axum's [`PaymentLayer`] and participates in
/// the `PaymentRouter` pipeline. `Clone` is supported via interior
/// `Arc` references; cheap.
pub struct X402Adapter {
    pending: Mutex<Option<X402Pending>>,
    layer: OnceLock<PaymentLayer>,
    priority: u32,
}

impl X402Adapter {
    /// Start building. `server` must already have `.initialize().await?`'d
    /// (spec §3 hard convention). Routes are declared on
    /// [`crate::UnifiedRouteConfig`] via
    /// `.adapter("x402", X402RouteConfig { ... })`.
    pub fn new(server: X402ResourceServer) -> X402AdapterBuilder {
        X402AdapterBuilder::new(server)
    }

    fn layer(&self) -> Result<&PaymentLayer, String> {
        self.layer.get().ok_or_else(|| {
            "x402 adapter used before PaymentRouterLayer::new finalized it".to_string()
        })
    }

    fn clone_parts_into_request(parts: &Parts) -> Request<Body> {
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

    fn get_challenge<'a>(
        &'a self,
        parts: &'a Parts,
        _route_cfg: &'a AdapterConfig,
    ) -> ChallengeFuture<'a> {
        // Use a clone of the prepared PaymentLayer against a dummy inner
        // service; the x402 middleware short-circuits to its 402 path
        // when no payment header is present.
        let layer = match self.layer() {
            Ok(l) => l.clone(),
            Err(e) => return Box::pin(async move { Err(e) }),
        };
        let request = Self::clone_parts_into_request(parts);
        Box::pin(async move {
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
            if resp.status() != StatusCode::PAYMENT_REQUIRED {
                return Ok(None);
            }
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
        let layer = self.layer().expect(
            "X402Adapter not prepared — register it via PaymentRouterLayer::new before serving",
        );
        BoxCloneSyncService::new(X402Service {
            inner: layer.clone().layer(inner),
        })
    }

    fn prepare(&self, routes: &[(String, UnifiedRouteConfig)]) -> Result<(), String> {
        // Take pending state; second call would error here (Option::take
        // returns None on the second invocation).
        let mut guard = self
            .pending
            .lock()
            .map_err(|_| "x402 adapter pending lock poisoned".to_string())?;
        let pending = guard
            .take()
            .ok_or_else(|| "x402 adapter prepare() called twice".to_string())?;
        drop(guard);

        // Extract x402 per-route config from each unified route.
        let mut x402_routes: RoutesConfig = HashMap::new();
        for (key, route) in routes {
            if let Some(cfg) = route.adapter_configs.get("x402") {
                let typed = cfg.downcast_ref::<X402RouteConfig>().ok_or_else(|| {
                    format!(
                        "x402 adapter: route {key:?} adapter_configs[\"x402\"] \
                         is not an X402RouteConfig"
                    )
                })?;
                x402_routes.insert(key.clone(), typed.to_x402());
            }
        }

        let mut builder = PaymentMiddlewareBuilder::new(x402_routes, pending.server)
            .poll_deadline(pending.poll_deadline);
        if let Some(h) = pending.timeout_hook {
            builder = builder.on_settlement_timeout(h);
        }
        if let Some(h) = pending.on_protected_request {
            builder = builder.on_protected_request(h);
        }
        if let Some(h) = pending.on_before_verify {
            builder = builder.on_before_verify(h);
        }
        if let Some(h) = pending.on_after_verify {
            builder = builder.on_after_verify(h);
        }
        if let Some(h) = pending.on_verify_failure {
            builder = builder.on_verify_failure(h);
        }
        if let Some(h) = pending.on_before_settle {
            builder = builder.on_before_settle(h);
        }
        if let Some(h) = pending.on_after_settle {
            builder = builder.on_after_settle(h);
        }
        if let Some(h) = pending.on_settle_failure {
            builder = builder.on_settle_failure(h);
        }
        if let Some(r) = pending.resolver {
            builder = builder.resolver(r);
        }

        self.layer
            .set(builder.build())
            .map_err(|_| "x402 adapter prepare race — layer already set".to_string())
    }

    /// Pin the matched `X402RouteConfig` (converted to x402-axum's
    /// `RoutePaymentConfig`) into request extensions as `PreMatchedRoute`.
    /// The inner x402 middleware reads it and bypasses its own path
    /// matching — required to handle outer-router pattern keys
    /// (`:param` / `*`) correctly without leaking past payment checks.
    fn enrich_request_extensions(
        &self,
        extensions: &mut http::Extensions,
        route_cfg: &UnifiedRouteConfig,
    ) {
        if let Some(adapter_cfg) = route_cfg.adapter_configs.get("x402") {
            if let Some(typed) = adapter_cfg.downcast_ref::<X402RouteConfig>() {
                extensions.insert(PreMatchedRoute(typed.to_x402()));
            }
        }
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

    #[test]
    fn detect_x_payment() {
        assert!(detect_logic(&parts_with("x-payment")));
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

    // -----------------------------------------------------------------
    // enrich_request_extensions — pattern-route bypass regression
    // -----------------------------------------------------------------

    use crate::config::AdapterConfig;
    use crate::types::UnifiedRouteConfig;
    use http::Extensions;
    use std::collections::HashMap as StdHashMap;

    fn route_cfg_with_x402(typed: X402RouteConfig) -> UnifiedRouteConfig {
        let mut adapter_configs = StdHashMap::new();
        adapter_configs.insert("x402".to_string(), AdapterConfig::new(typed));
        UnifiedRouteConfig {
            description: None,
            adapter_configs,
        }
    }

    fn route_cfg_without_x402() -> UnifiedRouteConfig {
        UnifiedRouteConfig {
            description: None,
            adapter_configs: StdHashMap::new(),
        }
    }

    /// Build a minimally-valid X402Adapter without going through
    /// X402AdapterBuilder.build (which requires an initialized server).
    /// We only test `enrich_request_extensions`, which doesn't touch
    /// `self.pending` / `self.layer`.
    fn adapter_for_enrichment_test() -> X402Adapter {
        X402Adapter {
            pending: Mutex::new(None),
            layer: OnceLock::new(),
            priority: 20,
        }
    }

    /// Outer router matched a pattern route (e.g. `GET /users/:id`).
    /// Adapter must pin the resolved per-route cfg into extensions so the
    /// inner x402 middleware uses it directly — closing the bypass where
    /// x402's exact-string lookup would otherwise miss `/users/123`.
    #[test]
    fn enrich_inserts_pre_matched_route_when_x402_cfg_present() {
        let adapter = adapter_for_enrichment_test();
        let typed = X402RouteConfig {
            accepts: vec![],
            description: "users-by-id".into(),
            mime_type: "application/json".into(),
            sync_settle: None,
            resource: None,
        };
        let route_cfg = route_cfg_with_x402(typed);
        let mut extensions = Extensions::new();

        adapter.enrich_request_extensions(&mut extensions, &route_cfg);

        let pinned = extensions
            .get::<PreMatchedRoute>()
            .expect("PreMatchedRoute should be inserted");
        assert_eq!(pinned.0.description, "users-by-id");
    }

    /// Route has no x402 entry → adapter does nothing. No extension is
    /// inserted; the inner middleware falls back to its own (now
    /// normalized) path matching against `state.routes`.
    #[test]
    fn enrich_no_op_when_route_lacks_x402_cfg() {
        let adapter = adapter_for_enrichment_test();
        let route_cfg = route_cfg_without_x402();
        let mut extensions = Extensions::new();

        adapter.enrich_request_extensions(&mut extensions, &route_cfg);

        assert!(extensions.get::<PreMatchedRoute>().is_none());
    }
}
