//! AggrDeferredEvmScheme — Server-side implementation of the "aggr_deferred" payment scheme.
//!
//! Mirrors: `@x402/mechanisms/evm/src/deferred/server/scheme.ts`
//!
//! The deferred scheme uses session key signing. The seller cannot submit the
//! signature on-chain (ecrecover ≠ from). Only the TEE can convert to an EOA
//! signature. From the seller's perspective, the scheme behaves identically to
//! "exact" for building payment requirements — the difference is handled
//! entirely by the Facilitator during verify/settle.

use async_trait::async_trait;

use x402_core::error::X402Error;
use x402_core::types::{AssetAmount, MoneyParser, Network, PaymentRequirements, Price, SupportedKind};
use x402_core::types::SchemeNetworkServer;

use crate::exact::ExactEvmScheme;

/// Server-side implementation of the "aggr_deferred" scheme.
///
/// Reuses ExactEvmScheme's price parsing and requirement building logic,
/// since the seller-side behavior is identical. The Facilitator handles
/// the session key → EOA signature conversion internally.
pub struct AggrDeferredEvmScheme {
    /// Delegate to ExactEvmScheme for shared logic.
    exact: ExactEvmScheme,
}

impl AggrDeferredEvmScheme {
    pub fn new() -> Self {
        Self {
            exact: ExactEvmScheme::new(),
        }
    }

    /// Register a custom money parser (delegates to inner ExactEvmScheme).
    pub fn register_money_parser(mut self, parser: MoneyParser) -> Self {
        self.exact = self.exact.register_money_parser(parser);
        self
    }
}

impl Default for AggrDeferredEvmScheme {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchemeNetworkServer for AggrDeferredEvmScheme {
    fn scheme(&self) -> &str {
        "aggr_deferred"
    }

    async fn parse_price(&self, price: &Price, network: &Network) -> Result<AssetAmount, X402Error> {
        self.exact.parse_price(price, network).await
    }

    async fn enhance_payment_requirements(
        &self,
        payment_requirements: PaymentRequirements,
        _supported_kind: &SupportedKind,
        _facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error> {
        Ok(payment_requirements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheme_name() {
        let scheme = AggrDeferredEvmScheme::new();
        assert_eq!(scheme.scheme(), "aggr_deferred");
    }

    #[tokio::test]
    async fn test_parse_price_xlayer() {
        let scheme = AggrDeferredEvmScheme::new();
        let price = Price::Money("$0.001".to_string());
        let result = scheme
            .parse_price(&price, &"eip155:196".to_string())
            .await
            .unwrap();

        assert_eq!(result.amount, "1000");
        assert_eq!(
            result.asset,
            "0x779ded0c9e1022225f8e0630b35a9b54be713736"
        );
    }
}
