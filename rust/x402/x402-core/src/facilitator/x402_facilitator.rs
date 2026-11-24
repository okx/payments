//! FacilitatorClient trait definition.

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

    /// Recover the payer by verifying ONLY the signature — no balance or nonce
    /// check. Used for the review-wallet exemption: the review test wallet
    /// holds no funds, so the normal balance-checking `verify` would reject it;
    /// and for aggr_deferred (session-key signing, `ecrecover != from`) the
    /// facilitator resolves the session key to the payer EOA, which the SDK
    /// cannot do locally. Returns `VerifyResponse` whose `payer` is the
    /// recovered signer.
    ///
    /// Calls `POST /api/v6/pay/x402/verify-signature`. Default: unsupported;
    /// the OKX HTTP client overrides it.
    ///
    /// Internal plumbing for `exempt_payers` — not developer-facing.
    #[doc(hidden)]
    async fn verify_signature_only(
        &self,
        _request: &VerifyRequest,
    ) -> Result<VerifyResponse, X402Error> {
        Err(X402Error::Other(
            "verify_signature_only not supported by this facilitator".to_string(),
        ))
    }

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
