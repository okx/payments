//! aggr_deferred payment scheme — OKX extension.
//!
//! Mirrors: `@x402/mechanisms/evm/src/aggr_deferred/`
//!
//! Session key based signing, OKX Facilitator batches on-chain asynchronously.

mod server_scheme;

pub use server_scheme::*;
