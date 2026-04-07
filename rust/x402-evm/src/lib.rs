//! x402-evm: EVM mechanism implementation for the x402 payment protocol.
//!
//! Mirrors: `@x402/mechanisms/evm` from the Coinbase x402 TypeScript SDK.
//! Extended with X Layer chain configuration and deferred scheme.
//!
//! # Modules
//!
//! - [`types`] — EVM-specific payload types (EIP-3009, Permit2)
//! - [`constants`] — Chain configs, default stablecoins, X Layer pre-registration
//! - [`exact`] — Exact scheme server implementation
//! - [`aggr_deferred`] — Deferred scheme server implementation (OKX extension)

pub mod types;
pub mod constants;
pub mod exact;
pub mod aggr_deferred;

// Re-export main types for convenience
pub use exact::ExactEvmScheme;
pub use aggr_deferred::AggrDeferredEvmScheme;
