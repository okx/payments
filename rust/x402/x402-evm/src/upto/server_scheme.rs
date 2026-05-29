//! Server-side implementation of the `"upto"` scheme.

use async_trait::async_trait;

use x402_core::error::X402Error;
use x402_core::types::SchemeNetworkServer;
use x402_core::types::{
    AssetAmount, MoneyParser, Network, PaymentRequirements, Price, SupportedKind,
};

use crate::exact::ExactEvmScheme;

pub struct UptoEvmScheme {
    exact: ExactEvmScheme,
}

impl UptoEvmScheme {
    pub fn new() -> Self {
        Self {
            exact: ExactEvmScheme::new(),
        }
    }

    pub fn register_money_parser(mut self, parser: MoneyParser) -> Self {
        self.exact = self.exact.register_money_parser(parser);
        self
    }
}

impl Default for UptoEvmScheme {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchemeNetworkServer for UptoEvmScheme {
    fn scheme(&self) -> &str {
        "upto"
    }

    async fn parse_price(
        &self,
        price: &Price,
        network: &Network,
    ) -> Result<AssetAmount, X402Error> {
        self.exact.parse_price(price, network).await
    }

    /// Force `assetTransferMethod = "permit2"` (upto is Permit2-only) and
    /// inject `facilitatorAddress` from `getSupported` so the buyer can pin
    /// it into `witness.facilitator`. Caller-pinned values are preserved.
    async fn enhance_payment_requirements(
        &self,
        mut payment_requirements: PaymentRequirements,
        supported_kind: &SupportedKind,
        _facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error> {
        payment_requirements.extra.insert(
            "assetTransferMethod".to_string(),
            serde_json::Value::String("permit2".to_string()),
        );

        if !payment_requirements
            .extra
            .contains_key("facilitatorAddress")
        {
            if let Some(facilitator_addr) = supported_kind
                .extra
                .as_ref()
                .and_then(|e| e.get("facilitatorAddress"))
                .cloned()
            {
                payment_requirements
                    .extra
                    .insert("facilitatorAddress".to_string(), facilitator_addr);
            }
        }

        Ok(payment_requirements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_requirements() -> PaymentRequirements {
        PaymentRequirements {
            scheme: "upto".to_string(),
            network: "eip155:196".to_string(),
            asset: "0x779ded0c9e1022225f8e0630b35a9b54be713736".to_string(),
            amount: "5000000".to_string(),
            pay_to: "0xMerchant".to_string(),
            max_timeout_seconds: 600,
            extra: HashMap::new(),
        }
    }

    fn make_supported_kind_with(facilitator: Option<&str>) -> SupportedKind {
        let extra = facilitator.map(|addr| {
            let mut m = HashMap::new();
            m.insert(
                "facilitatorAddress".to_string(),
                serde_json::Value::String(addr.to_string()),
            );
            m
        });
        SupportedKind {
            x402_version: 2,
            scheme: "upto".to_string(),
            network: "eip155:196".to_string(),
            extra,
        }
    }

    #[test]
    fn scheme_name_is_upto() {
        assert_eq!(UptoEvmScheme::new().scheme(), "upto");
    }

    #[tokio::test]
    async fn parse_price_delegates_to_exact() {
        let scheme = UptoEvmScheme::new();
        let result = scheme
            .parse_price(&Price::Money("$0.001".to_string()), &"eip155:196".to_string())
            .await
            .unwrap();
        assert_eq!(result.amount, "1000");
    }

    #[tokio::test]
    async fn enhance_forces_permit2_even_when_caller_set_eip3009() {
        let scheme = UptoEvmScheme::new();
        let mut req = make_requirements();
        req.extra.insert(
            "assetTransferMethod".to_string(),
            serde_json::Value::String("eip3009".to_string()),
        );
        let enhanced = scheme
            .enhance_payment_requirements(req, &make_supported_kind_with(Some("0xFacilitator")), &[])
            .await
            .unwrap();
        assert_eq!(
            enhanced.extra.get("assetTransferMethod"),
            Some(&serde_json::Value::String("permit2".to_string()))
        );
    }

    #[tokio::test]
    async fn enhance_injects_facilitator_address_from_supported_kind() {
        let scheme = UptoEvmScheme::new();
        let enhanced = scheme
            .enhance_payment_requirements(
                make_requirements(),
                &make_supported_kind_with(Some("0xFacilitatorAaa")),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(
            enhanced.extra.get("facilitatorAddress"),
            Some(&serde_json::Value::String("0xFacilitatorAaa".to_string()))
        );
    }

    #[tokio::test]
    async fn enhance_preserves_caller_set_facilitator_address() {
        let scheme = UptoEvmScheme::new();
        let mut req = make_requirements();
        req.extra.insert(
            "facilitatorAddress".to_string(),
            serde_json::Value::String("0xCallerPinned".to_string()),
        );
        let enhanced = scheme
            .enhance_payment_requirements(req, &make_supported_kind_with(Some("0xFacilitator")), &[])
            .await
            .unwrap();
        assert_eq!(
            enhanced.extra.get("facilitatorAddress"),
            Some(&serde_json::Value::String("0xCallerPinned".to_string()))
        );
    }

    #[tokio::test]
    async fn enhance_omits_facilitator_address_when_no_source_has_one() {
        let scheme = UptoEvmScheme::new();
        let enhanced = scheme
            .enhance_payment_requirements(make_requirements(), &make_supported_kind_with(None), &[])
            .await
            .unwrap();
        assert!(!enhanced.extra.contains_key("facilitatorAddress"));
    }
}
