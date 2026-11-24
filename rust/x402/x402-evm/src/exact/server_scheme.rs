//! ExactEvmScheme — Server-side implementation of the "exact" payment scheme.

use async_trait::async_trait;
use std::collections::HashMap;

use x402_core::error::X402Error;
use x402_core::types::SchemeNetworkServer;
use x402_core::types::{
    AssetAmount, MoneyParser, Network, PaymentRequirements, Price, SupportedKind,
};

use crate::constants::{get_default_asset, DefaultAssetInfo};

/// EVM server implementation for the "exact" payment scheme.
pub struct ExactEvmScheme {
    money_parsers: Vec<MoneyParser>,
}

impl ExactEvmScheme {
    pub fn new() -> Self {
        Self {
            money_parsers: Vec::new(),
        }
    }

    /// Register a custom money parser in the parser chain.
    ///
    /// Multiple parsers can be registered — they are tried in registration order.
    /// Each parser receives a decimal amount (e.g., 0.003 for "$0.003") and network.
    /// Return `Some(AssetAmount)` to handle, or `None` to pass to the next parser.
    /// The default conversion (using DEFAULT_STABLECOINS) is always the final fallback.
    pub fn register_money_parser(mut self, parser: MoneyParser) -> Self {
        self.money_parsers.push(parser);
        self
    }

    /// Strip currency symbols and whitespace from a money string.
    /// Returns the clean numeric string for string-based arithmetic.
    /// e.g., "$1.50" → "1.50", " 0.001 " → "0.001"
    fn parse_money_to_string(money: &str) -> Result<String, X402Error> {
        let clean = money.trim().trim_start_matches('$').trim().to_string();
        // Validate it's a valid, finite, non-negative number
        let value: f64 = clean
            .parse::<f64>()
            .map_err(|_| X402Error::PriceParse(format!("invalid money format: {}", money)))?;
        if !value.is_finite() {
            return Err(X402Error::PriceParse(format!(
                "money must be finite: {}",
                money
            )));
        }
        if value < 0.0 {
            return Err(X402Error::PriceParse(format!(
                "money must be non-negative: {}",
                money
            )));
        }
        Ok(clean)
    }

    /// Convert a decimal string amount to an integer token amount.
    ///
    /// Uses string-based arithmetic to avoid f64 precision issues.
    /// e.g., "0.001" with 6 decimals → "1000"
    fn convert_to_token_amount(decimal_amount: &str, decimals: u8) -> Result<String, X402Error> {
        // Work directly on the string to avoid f64 precision loss
        let parts: Vec<&str> = decimal_amount.split('.').collect();
        let int_part = parts[0];
        let dec_part = if parts.len() > 1 { parts[1] } else { "" };

        // Pad or truncate decimal part to match token decimals
        let padded_dec = if dec_part.len() >= decimals as usize {
            let truncated = &dec_part[decimals as usize..];
            if truncated.chars().any(|c| c != '0') {
                return Err(X402Error::PriceParse(format!(
                    "amount has {} decimal places but token only supports {}",
                    dec_part.len(),
                    decimals
                )));
            }
            &dec_part[..decimals as usize]
        } else {
            &format!("{:0<width$}", dec_part, width = decimals as usize)
        };

        let token_amount = format!("{}{}", int_part, padded_dec);
        // Strip leading zeros
        let token_amount = token_amount.trim_start_matches('0');
        if token_amount.is_empty() {
            Ok("0".to_string())
        } else {
            Ok(token_amount.to_string())
        }
    }

    /// Convert a numeric dollar amount to an AssetAmount using the default token.
    fn default_money_conversion(amount: &str, network: &str) -> Result<AssetAmount, X402Error> {
        let asset_info: DefaultAssetInfo = get_default_asset(network).ok_or_else(|| {
            X402Error::UnsupportedNetwork(format!(
                "no default asset configured for network {}",
                network
            ))
        })?;

        let token_amount = Self::convert_to_token_amount(amount, asset_info.decimals)?;

        // EIP-3009 tokens always need name/version for their transferWithAuthorization domain.
        // Permit2 tokens only need them if the token supports EIP-2612.
        let include_eip712_domain =
            asset_info.asset_transfer_method.is_none() || asset_info.supports_eip2612;

        let mut extra = HashMap::new();
        if include_eip712_domain {
            extra.insert(
                "name".to_string(),
                serde_json::Value::String(asset_info.name.to_string()),
            );
            extra.insert(
                "version".to_string(),
                serde_json::Value::String(asset_info.version.to_string()),
            );
        }
        if let Some(method) = asset_info.asset_transfer_method {
            extra.insert(
                "assetTransferMethod".to_string(),
                serde_json::Value::String(method.to_string()),
            );
        }

        Ok(AssetAmount {
            amount: token_amount,
            asset: asset_info.address.to_string(),
            extra: if extra.is_empty() { None } else { Some(extra) },
        })
    }
}

impl Default for ExactEvmScheme {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SchemeNetworkServer for ExactEvmScheme {
    fn scheme(&self) -> &str {
        "exact"
    }

    /// Parse a user-friendly price to an AssetAmount.
    async fn parse_price(
        &self,
        price: &Price,
        network: &Network,
    ) -> Result<AssetAmount, X402Error> {
        match price {
            // If already an AssetAmount, return it directly
            Price::Asset(asset_amount) => {
                if asset_amount.asset.is_empty() {
                    return Err(X402Error::PriceParse(format!(
                        "asset address must be specified for AssetAmount on network {}",
                        network
                    )));
                }
                Ok(asset_amount.clone())
            }
            // Parse Money string: try custom parsers first, then default conversion
            Price::Money(money) => {
                let amount_str = Self::parse_money_to_string(money)?;

                // Try custom money parsers in registration order
                let amount_f64: f64 = amount_str.parse().map_err(|_| {
                    X402Error::PriceParse(format!("invalid money format: {}", money))
                })?;
                for parser in &self.money_parsers {
                    if let Some(result) = parser(amount_f64, network) {
                        return Ok(result);
                    }
                }

                // All custom parsers returned None — use default conversion
                Self::default_money_conversion(&amount_str, network)
            }
        }
    }

    /// Backfill `extra.assetTransferMethod` from the network's default asset
    /// when callers build `PaymentRequirements` directly (bypassing
    /// `parse_price`). Otherwise pass through.
    async fn enhance_payment_requirements(
        &self,
        mut payment_requirements: PaymentRequirements,
        _supported_kind: &SupportedKind,
        _facilitator_extensions: &[String],
    ) -> Result<PaymentRequirements, X402Error> {
        if !payment_requirements.extra.contains_key("assetTransferMethod") {
            if let Some(asset_info) = get_default_asset(&payment_requirements.network) {
                if let Some(method) = asset_info.asset_transfer_method {
                    payment_requirements.extra.insert(
                        "assetTransferMethod".to_string(),
                        serde_json::Value::String(method.to_string()),
                    );
                }
            }
        }
        Ok(payment_requirements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parse_price_dollar_string() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("$0.001".to_string());
        let result = scheme
            .parse_price(&price, &"eip155:196".to_string())
            .await
            .unwrap();

        assert_eq!(result.amount, "1000");
        assert_eq!(result.asset, "0x779ded0c9e1022225f8e0630b35a9b54be713736");
    }

    #[tokio::test]
    async fn test_parse_price_plain_number() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("0.01".to_string());
        let result = scheme
            .parse_price(&price, &"eip155:196".to_string())
            .await
            .unwrap();

        assert_eq!(result.amount, "10000");
    }

    #[tokio::test]
    async fn test_parse_price_asset_amount() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Asset(AssetAmount {
            amount: "5000".to_string(),
            asset: "0xCustomToken".to_string(),
            extra: None,
        });
        let result = scheme
            .parse_price(&price, &"eip155:196".to_string())
            .await
            .unwrap();

        assert_eq!(result.amount, "5000");
        assert_eq!(result.asset, "0xCustomToken");
    }

    #[tokio::test]
    async fn test_parse_price_unsupported_network() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("$1.00".to_string());
        let result = scheme
            .parse_price(&price, &"eip155:99999".to_string())
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_convert_to_token_amount() {
        assert_eq!(
            ExactEvmScheme::convert_to_token_amount("0.001", 6).unwrap(),
            "1000"
        );
        assert_eq!(
            ExactEvmScheme::convert_to_token_amount("1.5", 6).unwrap(),
            "1500000"
        );
        assert_eq!(
            ExactEvmScheme::convert_to_token_amount("0", 6).unwrap(),
            "0"
        );
    }

    #[test]
    fn test_scheme_name() {
        let scheme = ExactEvmScheme::new();
        assert_eq!(scheme.scheme(), "exact");
    }

    fn make_requirements(network: &str, asset: &str) -> PaymentRequirements {
        PaymentRequirements {
            scheme: "exact".to_string(),
            network: network.to_string(),
            asset: asset.to_string(),
            amount: "1000".to_string(),
            pay_to: "0xMerchant".to_string(),
            max_timeout_seconds: 600,
            extra: HashMap::new(),
        }
    }

    fn make_supported_kind() -> SupportedKind {
        SupportedKind {
            x402_version: 2,
            scheme: "exact".to_string(),
            network: "eip155:196".to_string(),
            extra: None,
        }
    }

    #[tokio::test]
    async fn test_enhance_passthrough_when_extra_already_set() {
        // Caller already set extra.assetTransferMethod — must not be overwritten.
        let scheme = ExactEvmScheme::new();
        let mut req = make_requirements("eip155:196", "0xCustom");
        req.extra.insert(
            "assetTransferMethod".to_string(),
            serde_json::Value::String("eip3009".to_string()),
        );

        let enhanced = scheme
            .enhance_payment_requirements(req, &make_supported_kind(), &[])
            .await
            .unwrap();

        assert_eq!(
            enhanced.extra.get("assetTransferMethod"),
            Some(&serde_json::Value::String("eip3009".to_string()))
        );
    }

    #[tokio::test]
    async fn test_enhance_leaves_extra_alone_when_default_asset_has_no_method() {
        // X Layer USDT currently configures asset_transfer_method = None, so
        // enhance must not invent a value. The caller's explicit Price::Money
        // path (parse_price) is the right place to set Permit2 — this path
        // only safeguards direct PaymentRequirements construction.
        let scheme = ExactEvmScheme::new();
        let req = make_requirements("eip155:196", "0x779ded0c9e1022225f8e0630b35a9b54be713736");

        let enhanced = scheme
            .enhance_payment_requirements(req, &make_supported_kind(), &[])
            .await
            .unwrap();

        // Currently XLAYER_MAINNET_USDT has asset_transfer_method=None, so no
        // injection happens. If we later flip USDT to default Permit2 we need
        // to revisit this test.
        assert!(!enhanced.extra.contains_key("assetTransferMethod"));
    }
}
