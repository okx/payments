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
//! use std::{collections::HashMap, sync::Arc};
//! use axum::{Router, routing::get};
//! use payment_router_axum::{
//!     PaymentRouterConfig, PaymentRouterLayer, UnifiedRouteConfig,
//!     adapters::{MppAdapter, X402Adapter},
//! };
//!
//! let mpp_adapter = Arc::new(MppAdapter::new(mpp_challenger));
//! let x402_adapter = Arc::new(X402Adapter::builder(routes_config, x402_server).build());
//!
//! let app = Router::new()
//!     .route("/photo", get(|| async { "ok" }))
//!     .layer(PaymentRouterLayer::new(PaymentRouterConfig {
//!         routes: vec![("GET /photo".into(), UnifiedRouteConfig {
//!             description: Some("photo".into()),
//!             adapter_configs: HashMap::from([
//!                 ("mpp".into(),  serde_json::json!({"amount": "10000"})),
//!                 ("x402".into(), serde_json::json!({"scheme":"exact","price":"$0.01","network":"eip155:196","payTo":"0x..."})),
//!             ]),
//!         })],
//!         protocols: vec![mpp_adapter, x402_adapter],
//!         on_error: None,
//!     })?);
//! ```

pub mod adapter;
pub mod adapters;
pub mod detector;
pub mod layer;
pub mod merger;
pub mod router;
pub mod types;

pub use adapter::{ChallengeFuture, InnerService, ProtocolAdapter};
pub use layer::{PaymentRouterLayer, PaymentRouterService};
pub use router::BuildError;
pub use types::{ErrorContext, ErrorHandler, ErrorPhase, PaymentRouterConfig, UnifiedRouteConfig};
