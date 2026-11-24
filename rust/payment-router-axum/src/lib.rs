//! Dual-protocol (MPP + x402) payment router for axum.
//!
//! Implements the Adapter-pattern design from cross-language spec §1–§10.
//! Spec principles (verbatim):
//!
//! 1. **Adapter is a thin shell**: only detect / get_challenge /
//!    wrap-inner-with-native; the actual verify/settle is performed by
//!    each SDK's native middleware.
//! 2. **Zero intrusion**: x402 not patched, MPP not patched, no upstream
//!    PR required.
//! 3. **Router-level 402 synthesis**: parallel `get_challenge` →
//!    multi-row 402.
//! 4. **Ascending-priority serial detect** (first-match-wins), challenge
//!    generation runs concurrently.
//! 5. Cross-language behavior parity (verified by the conformance suite).
//!
//! # Quick start
//!
//! ```ignore
//! use std::sync::Arc;
//! use axum::{Router, routing::get};
//! use mpp_evm::{EvmCharge, EvmConfig, EvmMpp};
//! use payment_router_axum::{
//!     adapters::{MppAdapter, MppRouteConfig, X402Adapter, X402RouteConfig},
//!     PaymentRouterConfig, PaymentRouterLayer, ProtocolAdapter, UnifiedRouteConfig,
//! };
//! use x402_axum::AcceptConfig;
//!
//! // 1. EvmMpp facade — service config + charge (+ optional session).
//! let mpp = Arc::new(
//!     EvmMpp::builder(EvmConfig {
//!         chain_id: 196,
//!         recipient: "0x...".into(),
//!         secret_key: "hmac-secret".into(),
//!         realm: "shop".into(),
//!     })
//!     .with_charge(EvmCharge::new(sa_client).with_fee_payer(true))
//!     .build()?,
//! );
//! let mpp_adapter: Arc<dyn ProtocolAdapter> = Arc::new(MppAdapter::new(mpp));
//! let x402_adapter: Arc<dyn ProtocolAdapter> = Arc::new(X402Adapter::new(x402_server).build());
//!
//! // 2. Per-route typed configs — one place per adapter.
//! let route = UnifiedRouteConfig::builder()
//!     .description("photo")
//!     .adapter("mpp", MppRouteConfig {
//!         intent: "charge".into(),
//!         amount: "100".into(),
//!         currency: "0x...".into(),
//!         description: Some("photo".into()),
//!         external_id: None,
//!         unit_type: None,
//!         suggested_deposit: None,
//!     })
//!     .adapter("x402", X402RouteConfig {
//!         accepts: vec![AcceptConfig {
//!             scheme: "exact".into(), price: "$0.01".into(),
//!             network: "eip155:196".into(), pay_to: "0x...".into(),
//!             max_timeout_seconds: None, extra: None,
//!         }],
//!         description: "photo".into(),
//!         mime_type: "application/json".into(),
//!         sync_settle: None, resource: None,
//!     })
//!     .build();
//!
//! // 3. Layer + axum wiring.
//! let layer = PaymentRouterLayer::new(PaymentRouterConfig {
//!     routes: vec![("GET /photo".into(), route)],
//!     protocols: vec![mpp_adapter, x402_adapter],
//!     on_error: None,
//! })?;
//! let app = Router::new()
//!     .route("/photo", get(|| async { "ok" }))
//!     .layer(layer);
//! ```

pub mod adapter;
pub mod adapters;
pub mod config;
pub mod detector;
pub mod layer;
pub mod merger;
pub mod router;
pub mod types;

pub use adapter::{ChallengeFuture, InnerService, ProtocolAdapter};
pub use config::{AdapterConfig, UnifiedRouteConfigBuilder};
pub use layer::{PaymentRouterLayer, PaymentRouterService};
pub use router::BuildError;
pub use types::{ErrorContext, ErrorHandler, ErrorPhase, PaymentRouterConfig, UnifiedRouteConfig};
