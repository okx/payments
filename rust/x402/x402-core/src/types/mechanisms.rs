//! Mechanism traits for scheme/network implementations.
//!
//! Only `SchemeNetworkServer` is implemented (Seller SDK scope).
//! `SchemeNetworkClient` and `SchemeNetworkFacilitator` are omitted
//! (Client = Agentic Wallet CLI, Facilitator = OKX backend).

use async_trait::async_trait;

use super::{AssetAmount, Network, PaymentRequirements, Price, SupportedKind};
use crate::error::X402Error;

/// Custom money parser function.
///
/// Receives a decimal amount (e.g., 0.003 for "$0.003") and network identifier.
/// Returns `Some(AssetAmount)` to handle this price, or `None` to pass to the next parser.
pub type MoneyParser = Box<dyn Fn(f64, &str) -> Option<AssetAmount> + Send + Sync>;

/// When settlement happens relative to serving the protected resource.
///
/// Lets the middleware branch without hardcoding scheme names: `Post` schemes
/// (exact / aggr_deferred / upto) verify → serve → settle; `Pre` schemes
/// (`period`) settle first, then serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementMode {
    /// Verify before serving, settle after (the x402 default).
    Post,
    /// Settle (create / charge) before serving.
    Pre,
}

/// Server-side mechanism for a specific scheme/network combination.
/// Converts user-friendly prices to on-chain amounts and enhances payment requirements.
#[async_trait]
pub trait SchemeNetworkServer: Send + Sync {
    /// The payment scheme identifier (e.g., "exact", "aggr_deferred").
    fn scheme(&self) -> &str;

    /// Convert a user-friendly price to the scheme's specific amount and asset format.
    async fn parse_price(&self, price: &Price, network: &Network)
        -> Result<AssetAmount, X402Error>;

    /// Build payment requirements for this scheme/network combination.
    async fn enhance_payment_requirements(
        &self,
        payment_requirements: PaymentRequirements,
        supported_kind: &SupportedKind,
        facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error>;

    /// When this scheme settles relative to serving the resource. Defaults to
    /// [`SettlementMode::Post`]; `period` overrides to [`SettlementMode::Pre`].
    fn settlement_mode(&self) -> SettlementMode {
        SettlementMode::Post
    }
}
