//! aggr_deferred payment scheme — OKX extension.
//!
//! Session key based signing, OKX Facilitator batches on-chain asynchronously.

mod server_scheme;

pub use server_scheme::*;
