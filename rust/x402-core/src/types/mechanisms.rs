//! Mechanism traits for scheme/network implementations.
//!
//! Mirrors: `@x402/core/src/types/mechanisms.ts`
//!
//! Only `SchemeNetworkServer` is implemented (Seller SDK scope).
//! `SchemeNetworkClient` and `SchemeNetworkFacilitator` are omitted
//! (Client = Agentic Wallet CLI, Facilitator = OKX backend).

use async_trait::async_trait;

use super::{AssetAmount, Network, PaymentRequirements, Price, SupportedKind};
use crate::error::X402Error;

/// Server-side mechanism for a specific scheme/network combination.
/// Converts user-friendly prices to on-chain amounts and enhances payment requirements.
///
/// Mirrors TS: `export interface SchemeNetworkServer`
#[async_trait]
pub trait SchemeNetworkServer: Send + Sync {
    /// The payment scheme identifier (e.g., "exact", "aggr_deferred").
    fn scheme(&self) -> &str;

    /// Convert a user-friendly price to the scheme's specific amount and asset format.
    ///
    /// Mirrors TS: `parsePrice(price: Price, network: Network): Promise<AssetAmount>`
    async fn parse_price(&self, price: &Price, network: &Network) -> Result<AssetAmount, X402Error>;

    /// Build payment requirements for this scheme/network combination.
    ///
    /// Mirrors TS: `enhancePaymentRequirements(...): Promise<PaymentRequirements>`
    async fn enhance_payment_requirements(
        &self,
        payment_requirements: PaymentRequirements,
        supported_kind: &SupportedKind,
        facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error>;
}
