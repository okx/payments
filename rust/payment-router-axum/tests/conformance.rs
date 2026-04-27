//! Conformance test suite — spec §9 hard constraints + §10.10 translation
//! checklist.
//!
//! These tests go through `axum::serve` bound to a local random port and
//! assert behavior over HTTP. Unit tests (in `src/*.rs`) cover the same
//! constraints at the Tower Service boundary; this suite verifies the
//! end-to-end wire behavior (particularly multi-row headers, which depend
//! on hyper's serializer, not our code).

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::{routing::{get, post}, Json, Router};
use http::{HeaderMap, HeaderValue, header::WWW_AUTHENTICATE, request::Parts};
use payment_router_axum::{
    adapter::{ChallengeFuture, InnerService, ProtocolAdapter},
    PaymentRouterConfig, PaymentRouterLayer, UnifiedRouteConfig,
};
use serde_json::{json, Value};
use tower::util::BoxCloneSyncService;
use tower::Service;
use tower::ServiceExt as _;

// ---------------------------------------------------------------------------
// Fake adapters for controlled behavior
// ---------------------------------------------------------------------------

struct FakeAdapter {
    name: String,
    priority: u32,
    /// if true, `detect` always matches
    claim: bool,
    /// static challenge this adapter emits; None = Ok(None)
    challenge_header: Option<(String, String)>,
    /// optional hook counter incremented when make_service runs
    hook_hits: Arc<AtomicUsize>,
    /// if Some, `get_challenge` returns this error
    challenge_err: Option<String>,
}

impl ProtocolAdapter for FakeAdapter {
    fn name(&self) -> &str {
        &self.name
    }
    fn priority(&self) -> u32 {
        self.priority
    }
    fn detect(&self, _parts: &Parts) -> bool {
        self.claim
    }
    fn get_challenge<'a>(
        &'a self,
        _parts: &'a Parts,
        _route_cfg: &'a Value,
    ) -> ChallengeFuture<'a> {
        let err = self.challenge_err.clone();
        let header = self.challenge_header.clone();
        Box::pin(async move {
            if let Some(msg) = err {
                return Err(msg);
            }
            match header {
                Some((name, val)) => {
                    let mut map = HeaderMap::new();
                    map.append(
                        http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                        HeaderValue::from_str(&val).unwrap(),
                    );
                    Ok(Some(map))
                }
                None => Ok(None),
            }
        })
    }
    fn make_service(&self, inner: InnerService) -> InnerService {
        let name = self.name.clone();
        let hits = self.hook_hits.clone();
        BoxCloneSyncService::new(tower::service_fn(move |req: http::Request<axum::body::Body>| {
            let mut inner = inner.clone();
            let name = name.clone();
            let hits = hits.clone();
            async move {
                // hook: increment counter (analogous to x402 onAfterSettle)
                hits.fetch_add(1, Ordering::SeqCst);
                use tower::ServiceExt;
                let mut resp = inner.ready().await?.call(req).await?;
                resp.headers_mut()
                    .insert("x-handled-by", HeaderValue::from_str(&name).unwrap());
                Ok::<_, std::convert::Infallible>(resp)
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// Harness: launch a test server on random port, return base URL
// ---------------------------------------------------------------------------

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn cfg(adapters: &[&str]) -> UnifiedRouteConfig {
    let mut m = HashMap::new();
    for a in adapters {
        m.insert((*a).into(), json!({}));
    }
    UnifiedRouteConfig {
        description: None,
        adapter_configs: m,
    }
}

// ---------------------------------------------------------------------------
// §9 route priority: first-match-wins
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case01_priority_short_circuit() {
    // MPP priority 10 claims → x402 detect should NOT be queried. We assert
    // this by recording hook_hits on x402 and verifying it stays 0.
    let mpp_hits = Arc::new(AtomicUsize::new(0));
    let x402_hits = Arc::new(AtomicUsize::new(0));
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: true,
        challenge_header: None,
        hook_hits: mpp_hits.clone(),
        challenge_err: None,
    });
    let x402: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "x402".into(),
        priority: 20,
        claim: true,
        challenge_header: None,
        hook_hits: x402_hits.clone(),
        challenge_err: None,
    });
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /x".into(), cfg(&["mpp", "x402"]))],
                protocols: vec![mpp, x402],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/x")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-handled-by").unwrap().to_str().unwrap(),
        "mpp"
    );
    assert_eq!(mpp_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        x402_hits.load(Ordering::SeqCst),
        0,
        "x402 must not be invoked when mpp (higher priority) claimed"
    );
}

// ---------------------------------------------------------------------------
// §10 challenge concurrency + single-adapter error isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case02_single_challenge_failure_does_not_block() {
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: false,
        challenge_header: Some(("WWW-Authenticate".into(), "Payment realm=\"m\"".into())),
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let x402: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "x402".into(),
        priority: 20,
        claim: false,
        challenge_header: None,
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: Some("simulated x402 failure".into()),
    });
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /x".into(), cfg(&["mpp", "x402"]))],
                protocols: vec![mpp, x402],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/x")).await.unwrap();
    assert_eq!(resp.status(), 402);
    let rows: Vec<_> = resp
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "mpp still contributes even though x402 errored"
    );
    assert!(rows[0].contains("Payment realm"));
}

// ---------------------------------------------------------------------------
// §9 multi-row WWW-Authenticate: verify raw bytes over the wire (not
// comma-concatenated by curl or reqwest).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case05_multi_row_www_authenticate() {
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: false,
        challenge_header: Some(("WWW-Authenticate".into(), "Payment realm=\"m\"".into())),
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let x402: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "x402".into(),
        priority: 20,
        claim: false,
        challenge_header: Some(("WWW-Authenticate".into(), "x402 realm=\"x\"".into())),
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /x".into(), cfg(&["mpp", "x402"]))],
                protocols: vec![mpp, x402],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/x")).await.unwrap();
    assert_eq!(resp.status(), 402);
    let rows: Vec<_> = resp
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(rows.len(), 2, "spec §3 #3 / §9: no comma concat");
    assert!(rows.iter().any(|r| r.contains("Payment realm")));
    assert!(rows.iter().any(|r| r.contains("x402 realm")));
}

// ---------------------------------------------------------------------------
// §2 non-payment route passes through unaffected (no 402, no header injection)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case06_bypass_non_paid_route() {
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: true,
        challenge_header: None,
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let app = Router::new()
        .route("/paid", get(|| async { "paid" }))
        .route("/free", get(|| async { "free" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /paid".into(), cfg(&["mpp"]))],
                protocols: vec![mpp],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/free")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "free");
}

// ---------------------------------------------------------------------------
// §3 #1 detect does not consume body — POST with body and no auth must
// pass the body through to the handler after 402 retry with auth.
// We simulate: handler echoes body length. Without auth → 402 (body
// discarded). With auth (via fake adapter claiming) → echoes length.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case08_body_not_consumed_by_detect() {
    let hits = Arc::new(AtomicUsize::new(0));
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: true, // unconditionally claim — exercises make_service path
        challenge_header: None,
        hook_hits: hits.clone(),
        challenge_err: None,
    });
    let app = Router::new()
        .route(
            "/echo",
            post(|body: String| async move { Json(json!({"len": body.len()})) }),
        )
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("POST /echo".into(), cfg(&["mpp"]))],
                protocols: vec![mpp],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/echo"))
        .body("hello-world")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["len"], 11, "body reached handler intact");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------
// §3 #6 adapter hooks run on the real handler response (not on a synthetic
// dummy). Verifies Route A principle: wrapped service drives the real inner.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case09_hooks_see_real_handler_response() {
    let hits = Arc::new(AtomicUsize::new(0));
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: true,
        challenge_header: None,
        hook_hits: hits.clone(),
        challenge_err: None,
    });
    let app = Router::new()
        .route(
            "/real",
            get(|| async {
                // Some distinctive marker the fake adapter's hook can observe
                // via response headers injection (x-handled-by).
                "real-handler-ran"
            }),
        )
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /real".into(), cfg(&["mpp"]))],
                protocols: vec![mpp],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/real")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-handled-by").unwrap().to_str().unwrap(),
        "mpp",
        "adapter wrapper injected header after real handler ran"
    );
    assert_eq!(resp.text().await.unwrap(), "real-handler-ran");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "hook fired exactly once");
}

// ---------------------------------------------------------------------------
// §3 #4 adapter with no config for this route is skipped in challenge merge
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case04_adapter_no_config_skipped_in_merge() {
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: false,
        challenge_header: Some(("WWW-Authenticate".into(), "Payment realm=\"m\"".into())),
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let x402: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "x402".into(),
        priority: 20,
        claim: false,
        challenge_header: Some(("WWW-Authenticate".into(), "x402 realm=\"x\"".into())),
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    // route only has "mpp" config — x402 adapter is registered but its
    // get_challenge shouldn't run for this route.
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /x".into(), cfg(&["mpp"]))],
                protocols: vec![mpp, x402],
                on_error: None,
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let resp = reqwest::get(format!("{base}/x")).await.unwrap();
    assert_eq!(resp.status(), 402);
    let rows: Vec<_> = resp
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("Payment realm"));
}

// ---------------------------------------------------------------------------
// §9 fail-fast on unknown adapter key in route config (build-time)
// ---------------------------------------------------------------------------

#[test]
fn case_build_rejects_unknown_adapter() {
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: false,
        challenge_header: None,
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: None,
    });
    let err = PaymentRouterLayer::new(PaymentRouterConfig {
        routes: vec![("GET /x".into(), cfg(&["lightning"]))],
        protocols: vec![mpp],
        on_error: None,
    });
    assert!(err.is_err(), "unknown adapter key must fail-fast");
}

// ---------------------------------------------------------------------------
// §9 on_error called when a challenge adapter errors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn case07_on_error_invoked_on_challenge_failure() {
    let calls: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let calls_clone = calls.clone();
    let mpp: Arc<dyn ProtocolAdapter> = Arc::new(FakeAdapter {
        name: "mpp".into(),
        priority: 10,
        claim: false,
        challenge_header: None,
        hook_hits: Arc::new(AtomicUsize::new(0)),
        challenge_err: Some("boom".into()),
    });
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(
            PaymentRouterLayer::new(PaymentRouterConfig {
                routes: vec![("GET /x".into(), cfg(&["mpp"]))],
                protocols: vec![mpp],
                on_error: Some(Arc::new(move |err, ctx| {
                    calls_clone
                        .lock()
                        .unwrap()
                        .push(format!("{} {} {}", ctx.phase.as_str(), ctx.protocol, err));
                })),
            })
            .unwrap(),
        );
    let base = serve(app).await;
    let _ = reqwest::get(format!("{base}/x")).await.unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].starts_with("challenge mpp "));
    assert!(calls[0].contains("boom"));
}

#[allow(dead_code)]
fn _type_check_future_shape() -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    // Ensures ChallengeFuture shape compiles with the trait bounds used in
    // PaymentRouterService (Send + 'static on the erased futures).
    Box::pin(async {})
}
