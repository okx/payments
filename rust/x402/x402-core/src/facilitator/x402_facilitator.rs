//! FacilitatorClient trait definition.
//!
//! Mirrors: `@x402/core/src/facilitator/x402Facilitator.ts`

use async_trait::async_trait;

use crate::error::X402Error;
use crate::types::{
    SettleRequest, SettleResponse, SettleStatusResponse, SupportedResponse, VerifyRequest,
    VerifyResponse,
};

/// Trait for communicating with a remote x402 facilitator.
///
/// In Coinbase TS, this is the `HTTPFacilitatorClient` class.
/// The trait abstraction allows different implementations
/// (e.g., OKX with HMAC signing, Coinbase standard, mock for testing).
#[async_trait]
pub trait FacilitatorClient: Send + Sync {
    /// Query the facilitator's supported schemes, networks, and extensions.
    ///
    /// Calls: `GET /supported`
    async fn get_supported(&self) -> Result<SupportedResponse, X402Error>;

    /// Verify a payment authorization without executing on-chain.
    ///
    /// Calls: `POST /verify`
    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, X402Error>;

    /// Submit a payment for on-chain settlement.
    ///
    /// Calls: `POST /settle`
    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, X402Error>;

    /// Query on-chain settlement status by transaction hash.
    ///
    /// Calls: `GET /settle/status?txHash=...`
    ///
    /// OKX extension: used for async polling (exact syncSettle=false)
    /// and deferred scheme on-chain status tracking.
    async fn get_settle_status(&self, tx_hash: &str) -> Result<SettleStatusResponse, X402Error>;
}
