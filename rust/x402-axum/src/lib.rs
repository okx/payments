//! x402-axum: Axum middleware adapter for the x402 payment protocol.
//!
//! Mirrors: `@x402/http/express` from the Coinbase x402 TypeScript SDK.
//!
//! Provides Tower Layer/Service middleware that integrates x402-core's
//! payment processing into Axum applications.
//!
//! # Usage
//!
//! ```rust,no_run
//! use x402_axum::payment_middleware;
//! use x402_core::server::X402ResourceServer;
//! use x402_core::http::RoutesConfig;
//!
//! // let server = X402ResourceServer::new(facilitator_client)...
//! // let routes: RoutesConfig = HashMap::from([...]);
//! // let app = Router::new()
//! //     .route("/weather", get(handler))
//! //     .layer(payment_middleware(routes, server));
//! ```

mod adapter;
mod middleware;

pub use adapter::*;
pub use middleware::*;

// Re-export commonly used types from x402-core
pub use x402_core::http::{
    AcceptConfig, BeforeHookResult, OnAfterSettleHook, OnAfterVerifyHook, OnBeforeSettleHook,
    OnBeforeVerifyHook, OnProtectedRequestHook, OnSettleFailureHook, OnSettlementTimeoutHook,
    OnVerifyFailureHook, PaymentResolverFn, PollResult, ProtectedRequestResult, RequestContext,
    ResolvedAccept, RoutePaymentConfig, RoutesConfig, SettleContext, SettleRecoveryResult,
    SettleResultContext, SettlementTimeoutResult, VerifyContext, VerifyRecoveryResult,
    VerifyResultContext, DEFAULT_POLL_DEADLINE, DEFAULT_POLL_INTERVAL,
};
pub use x402_core::server::X402ResourceServer;
