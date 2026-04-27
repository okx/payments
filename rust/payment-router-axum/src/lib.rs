//! Dual-protocol (MPP + x402) payment router for axum.
//!
//! Implements the Adapter-pattern design from cross-language spec §1–§10.
//! Spec principles (verbatim):
//!
//! 1. **Adapter 是薄壳**: only detect / get_challenge / wrap-inner-with-native;
//!    真正的 verify/settle 由各 SDK 原生中间件完成。
//! 2. **零侵入**: x402 not patched, MPP not patched, no upstream PR required.
//! 3. **paymentrouter 层自产 402**: parallel `get_challenge` → multi-row 402.
//! 4. **priority 升序串行 detect** (first-match-wins), challenge 生成并发。
//! 5. 行为跨语言完全一致 (通过 conformance 验收)。
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
