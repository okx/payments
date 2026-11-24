//! Unified error types for the x402 SDK.

use crate::types::{FacilitatorResponseError, SettleError, VerifyError};

/// Top-level error type for x402 operations.
#[derive(Debug, thiserror::Error)]
pub enum X402Error {
    #[error(transparent)]
    Verify(#[from] VerifyError),

    #[error(transparent)]
    Settle(#[from] SettleError),

    #[error(transparent)]
    FacilitatorResponse(#[from] FacilitatorResponseError),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("route configuration error: {0}")]
    RouteConfig(String),

    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),

    #[error("unsupported network: {0}")]
    UnsupportedNetwork(String),

    #[error("price parse error: {0}")]
    PriceParse(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    #[error("not initialized: {0}")]
    NotInitialized(String),

    #[error("{0}")]
    Other(String),
}
