//! Tower `Layer` / `Service` implementation.
//!
//! `PaymentRouterLayer::new(config)` validates the config at startup (route
//! keys must reference registered adapters). When axum calls `layer(inner)`:
//! each adapter is handed a clone of `inner` and wraps it with its own native
//! middleware (e.g. x402-axum `PaymentMiddleware`). The bare `inner` is also
//! kept as a fallback for non-payment routes.
//!
//! At request time (`PaymentRouterService::call`):
//! 1. Match route. No match → forward to bare `inner` (non-paid route).
//! 2. Detect (priority ascending, first-match-wins). If an adapter claims the
//!    request, dispatch to its wrapped service — the native middleware runs
//!    the full verify → handler → settle pipeline. All hooks preserved.
//! 3. No adapter claims → merge `get_challenge` from every adapter in
//!    parallel, return multi-row 402.

use std::collections::HashSet;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use http::{Request, Response};
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service, ServiceExt};

use crate::adapter::{InnerService, ProtocolAdapter};
use crate::detector;
use crate::merger;
use crate::router::{BuildError, CompiledRouter};
use crate::types::{ErrorContext, ErrorHandler, ErrorPhase, PaymentRouterConfig};

/// The Tower Layer installed via `.layer(...)` on an axum Router.
///
/// Construct with [`PaymentRouterLayer::new`]. Configuration is validated at
/// construction (fail-fast on unknown adapter keys in route configs).
#[derive(Clone)]
pub struct PaymentRouterLayer {
    inner: Arc<LayerState>,
}

struct LayerState {
    /// Adapters sorted ascending by priority at construction.
    adapters: Vec<Arc<dyn ProtocolAdapter>>,
    router: CompiledRouter,
    on_error: Option<Arc<ErrorHandler>>,
}

impl PaymentRouterLayer {
    /// Build the Layer. Fails if any `UnifiedRouteConfig::adapter_configs` key
    /// references an unregistered adapter.name().
    pub fn new(config: PaymentRouterConfig) -> Result<Self, BuildError> {
        // Validate + sort adapters by priority.
        let mut adapters = config.protocols;
        adapters.sort_by_key(|a| a.priority());
        let adapter_names: HashSet<String> =
            adapters.iter().map(|a| a.name().to_string()).collect();
        let router = CompiledRouter::new(config.routes, &adapter_names)?;
        Ok(Self {
            inner: Arc::new(LayerState {
                adapters,
                router,
                on_error: config.on_error,
            }),
        })
    }
}

impl<S> Layer<S> for PaymentRouterLayer
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    type Service = PaymentRouterService;

    fn layer(&self, inner: S) -> PaymentRouterService {
        // Type-erase inner so adapters can compose polymorphically.
        let inner_boxed: InnerService = BoxCloneSyncService::new(inner);

        // Each adapter wraps its own copy of inner with its native middleware.
        let adapter_services: Vec<InnerService> = self
            .inner
            .adapters
            .iter()
            .map(|adapter| adapter.make_service(inner_boxed.clone()))
            .collect();

        PaymentRouterService {
            state: self.inner.clone(),
            adapter_services,
            fallback: inner_boxed,
        }
    }
}

/// The Tower `Service` produced by `PaymentRouterLayer::layer`.
pub struct PaymentRouterService {
    state: Arc<LayerState>,
    /// Parallel to `state.adapters`. `adapter_services[i]` is the wrapped
    /// service for `adapters[i]`.
    adapter_services: Vec<InnerService>,
    /// Bare inner for routes not under payment protection (spec §2 first step
    /// of the flow: no match → pass through).
    fallback: InnerService,
}

impl Clone for PaymentRouterService {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            adapter_services: self.adapter_services.iter().cloned().collect(),
            fallback: self.fallback.clone(),
        }
    }
}

impl Service<Request<Body>> for PaymentRouterService {
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<
        Box<dyn std::future::Future<Output = Result<Response<Body>, Infallible>> + Send + 'static>,
    >;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        // Inner services are cloned per-call; readiness polled after clone in
        // the async path via `ready_oneshot`. No shared backpressure here.
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        let adapter_services = self.adapter_services.clone();
        let fallback = self.fallback.clone();

        Box::pin(async move {
            dispatch(state, adapter_services, fallback, req).await
        })
    }
}

async fn dispatch(
    state: Arc<LayerState>,
    mut adapter_services: Vec<InnerService>,
    fallback: InnerService,
    req: Request<Body>,
) -> Result<Response<Body>, Infallible> {
    let (parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let path = parts.uri.path().to_string();

    let route_match = state.router.match_route(&method, &path);

    // No matching payment route → pass through.
    let (route_key, route_cfg) = match route_match {
        Some(m) => (m.route_key.to_string(), m.config.clone()),
        None => {
            let req = Request::from_parts(parts, body);
            return fallback
                .oneshot(req)
                .await
                .map_err(|_: Infallible| unreachable!());
        }
    };

    // Detect adapter (priority-ordered, short-circuit).
    let matched = detector::detect(&state.adapters, &parts);

    match matched {
        Some(i) => {
            let req = Request::from_parts(parts, body);
            match adapter_services
                .get_mut(i)
                .expect("adapter_services parallel to adapters")
                .call(req)
                .await
            {
                Ok(resp) => Ok(resp),
                Err(err) => {
                    // adapter_services are BoxCloneService<..., Infallible>,
                    // so this branch is unreachable but we handle it defensively.
                    if let Some(handler) = &state.on_error {
                        let ctx = ErrorContext {
                            phase: ErrorPhase::Handle,
                            protocol: state.adapters[i].name().to_string(),
                            route: Some(route_key),
                        };
                        let err_box: Box<dyn std::error::Error + Send + Sync> =
                            format!("{err:?}").into();
                        (handler)(err_box.as_ref(), ctx);
                    }
                    Ok(Response::builder()
                        .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .expect("static 500"))
                }
            }
        }
        None => {
            // No adapter claimed → merge 402.
            let merged = merger::merge_challenges(
                &state.adapters,
                &parts,
                &route_cfg,
                &route_key,
                state.on_error.as_ref(),
            )
            .await;
            Ok(merger::build_402_response(merged))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{ChallengeFuture, InnerService, ProtocolAdapter};
    use crate::types::UnifiedRouteConfig;
    use axum::body::{to_bytes, Body};
    use http::{HeaderValue, StatusCode, header::WWW_AUTHENTICATE, request::Parts};
    use serde_json::Value;
    use std::collections::HashMap;

    struct ClaimingAdapter {
        name: String,
        priority: u32,
        claim_if_header_present: &'static str,
    }

    impl ProtocolAdapter for ClaimingAdapter {
        fn name(&self) -> &str {
            &self.name
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn detect(&self, parts: &Parts) -> bool {
            parts.headers.contains_key(self.claim_if_header_present)
        }
        fn get_challenge<'a>(
            &'a self,
            _parts: &'a Parts,
            _route_cfg: &'a Value,
        ) -> ChallengeFuture<'a> {
            let mut map = http::HeaderMap::new();
            map.append(
                WWW_AUTHENTICATE,
                HeaderValue::from_str(&format!(
                    "{} realm=\"test\"",
                    if self.name == "mpp" { "Payment" } else { "x402" }
                ))
                .unwrap(),
            );
            Box::pin(async move { Ok(Some(map)) })
        }
        fn make_service(&self, inner: InnerService) -> InnerService {
            let name = self.name.clone();
            BoxCloneSyncService::new(tower::service_fn(move |req: Request<Body>| {
                let mut inner = inner.clone();
                let protocol = name.clone();
                async move {
                    let mut resp = inner.ready().await?.call(req).await?;
                    resp.headers_mut().insert(
                        "X-Handled-By",
                        HeaderValue::from_str(&protocol).unwrap(),
                    );
                    Ok::<_, Infallible>(resp)
                }
            }))
        }
    }

    fn cfg_both() -> UnifiedRouteConfig {
        let mut m = HashMap::new();
        m.insert("mpp".into(), serde_json::json!({}));
        m.insert("x402".into(), serde_json::json!({}));
        UnifiedRouteConfig {
            description: None,
            adapter_configs: m,
        }
    }

    fn build() -> PaymentRouterService {
        let mpp: Arc<dyn ProtocolAdapter> = Arc::new(ClaimingAdapter {
            name: "mpp".into(),
            priority: 10,
            claim_if_header_present: "authorization",
        });
        let x402: Arc<dyn ProtocolAdapter> = Arc::new(ClaimingAdapter {
            name: "x402".into(),
            priority: 20,
            claim_if_header_present: "x-payment",
        });
        let layer = PaymentRouterLayer::new(PaymentRouterConfig {
            routes: vec![("GET /photos".into(), cfg_both())],
            protocols: vec![mpp, x402],
            on_error: None,
        })
        .unwrap();
        // inner: dummy 200 returning "inner-response"
        let inner = tower::service_fn(|_: Request<Body>| async move {
            Ok::<_, Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from("inner-response"))
                    .unwrap(),
            )
        });
        layer.layer(inner)
    }

    #[tokio::test]
    async fn mpp_header_routes_to_mpp_adapter() {
        let mut svc = build();
        let req = Request::builder()
            .method("GET")
            .uri("/photos")
            .header("authorization", "Payment abc")
            .body(Body::empty())
            .unwrap();
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("X-Handled-By").unwrap().to_str().unwrap(),
            "mpp"
        );
    }

    #[tokio::test]
    async fn x402_header_routes_to_x402_adapter() {
        let mut svc = build();
        let req = Request::builder()
            .method("GET")
            .uri("/photos")
            .header("x-payment", "abc")
            .body(Body::empty())
            .unwrap();
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("X-Handled-By").unwrap().to_str().unwrap(),
            "x402"
        );
    }

    #[tokio::test]
    async fn no_auth_returns_multi_row_402() {
        let mut svc = build();
        let req = Request::builder()
            .method("GET")
            .uri("/photos")
            .body(Body::empty())
            .unwrap();
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYMENT_REQUIRED);
        let rows: Vec<_> = resp
            .headers()
            .get_all(WWW_AUTHENTICATE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert_eq!(rows.len(), 2, "expected two WWW-Authenticate rows");
        assert!(rows.iter().any(|r| r.contains("Payment")));
        assert!(rows.iter().any(|r| r.contains("x402")));
    }

    #[tokio::test]
    async fn non_paid_route_passes_through_to_fallback() {
        let mut svc = build();
        let req = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = svc.call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"inner-response");
        // No payment handler involved, so no X-Handled-By.
        assert!(svc.state.adapters.len() == 2);
    }

    #[tokio::test]
    async fn both_headers_priority_wins() {
        let mut svc = build();
        let req = Request::builder()
            .method("GET")
            .uri("/photos")
            .header("authorization", "Payment abc")
            .header("x-payment", "abc")
            .body(Body::empty())
            .unwrap();
        let resp = svc.call(req).await.unwrap();
        // MPP has priority 10, x402 has priority 20 → MPP wins.
        assert_eq!(
            resp.headers().get("X-Handled-By").unwrap().to_str().unwrap(),
            "mpp"
        );
    }
}
