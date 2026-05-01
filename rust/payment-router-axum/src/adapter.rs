//! `ProtocolAdapter` trait — spec §3 thin-shell adapter.
//!
//! Each adapter does three things:
//!
//! 1. `detect(parts)` — pure sync, header-only check. Claims the request for
//!    its protocol. First adapter to return true (by priority) wins; later
//!    adapters are not queried.
//! 2. `get_challenge(parts, route_cfg)` — async, generates this protocol's 402
//!    challenge headers. Called in parallel across all adapters when no adapter
//!    detected (merged into a multi-row WWW-Authenticate 402).
//! 3. `make_service(inner)` — given the real inner axum Router (as a boxed,
//!    clone-able Tower service), returns a wrapped service that runs this
//!    protocol's native verify/handler/settle pipeline. For x402 this is
//!    literally `PaymentMiddleware(inner)` — all hooks / resolver / timeout
//!    recovery preserved natively. For MPP this is a thin wrapper that calls
//!    `ChargeChallenger::verify_payment` → `inner` → appends `Payment-Receipt`.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;

use axum::body::{Body, Bytes};
use http::{request::Parts, HeaderMap, Request, Response};
use serde_json::Value;
use tower::util::BoxCloneSyncService;

/// Type-erased inner service used by all adapters.
///
/// axum's `Router` is `Service<Request<Body>, Response = Response<Body>, Error = Infallible>`
/// and `Clone`. We erase it to `BoxCloneSyncService` (Sync-capable) because
/// axum's own `Router::layer` requires the produced Service to be Sync
/// (`L::Service: Service<Request> + Clone + Send + Sync + 'static`).
/// `BoxCloneService` is only Send+!Sync and would fail the axum trait bound.
pub type InnerService = BoxCloneSyncService<Request<Body>, Response<Body>, Infallible>;

/// Output of `ProtocolAdapter::get_challenge`. Carries both the headers
/// the adapter wants merged into the 402 response, and an optional body.
///
/// **Body handling**: x402 spec requires its 402 to carry an `accepts[]`
/// array in the response body. MPP carries everything in headers and
/// expects an RFC 9457 problem+json body (`{type, title, status}`). When
/// multiple adapters contribute bodies, [`merge_challenges`] picks the
/// first non-empty one (lower-priority adapters win), so the merged
/// response stays interoperable with x402 clients.
#[derive(Debug, Clone, Default)]
pub struct ChallengeResponse {
    pub headers: HeaderMap,
    /// Optional response body bytes. `None` means "use the merger's
    /// default RFC 9457 problem+json body".
    pub body: Option<Bytes>,
}

impl ChallengeResponse {
    /// Convenience: header-only response (MPP-style).
    pub fn headers_only(headers: HeaderMap) -> Self {
        Self {
            headers,
            body: None,
        }
    }
}

/// Future returned by `ProtocolAdapter::get_challenge`.
///
/// Lifetime `'a` is tied to `&Parts` / `&Value` — the future cannot outlive
/// those borrows. In practice `merger` awaits `join_all` before either goes out
/// of scope.
pub type ChallengeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<ChallengeResponse>, String>> + Send + 'a>>;

/// Spec §3 ProtocolAdapter. Implementors must be `Send + Sync + 'static`
/// because `Arc<dyn ProtocolAdapter>` is cloned into adapter services and
/// shared across tokio tasks.
pub trait ProtocolAdapter: Send + Sync + 'static {
    /// Protocol name (e.g. `"mpp"` / `"x402"`). Used as the key into
    /// `UnifiedRouteConfig::adapter_configs`.
    fn name(&self) -> &str;

    /// Detect scheduling priority. Smaller = higher priority = detected first.
    /// Built-in MPP=10, x402=20. User extensions start from 100 per spec §9.
    fn priority(&self) -> u32;

    /// Pure-sync, header-only check. Spec §3 #1: must not consume body.
    /// Spec §3 #6: should not panic; panics are treated as miss by the caller.
    fn detect(&self, parts: &Parts) -> bool;

    /// Generate this protocol's 402 challenge headers.
    ///
    /// `route_cfg` is the adapter-specific slice of `UnifiedRouteConfig::adapter_configs`
    /// (JSON value). Each adapter deserializes its own shape.
    ///
    /// Returns:
    /// - `Ok(Some(headers))` → add to merged 402
    /// - `Ok(None)` → skip this adapter (e.g. no config for this route)
    /// - `Err(msg)` → reported via `on_error`, treated as empty for merging
    fn get_challenge<'a>(&'a self, parts: &'a Parts, route_cfg: &'a Value) -> ChallengeFuture<'a>;

    /// Wrap the inner service with this protocol's native middleware.
    ///
    /// Returned service is called only when `detect` returned `true` for the
    /// incoming request. Implementations must delegate to the native SDK
    /// middleware (x402-axum `PaymentMiddleware`, mpp-evm `EvmChargeChallenger`)
    /// — spec §1 principle "Adapter is a thin shell".
    fn make_service(&self, inner: InnerService) -> InnerService;
}
