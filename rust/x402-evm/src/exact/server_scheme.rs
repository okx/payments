//! ExactEvmScheme — Server-side implementation of the "exact" payment scheme.
//!
//! Mirrors: `@x402/mechanisms/evm/src/exact/server/scheme.ts`

use async_trait::async_trait;
use std::collections::HashMap;

use x402_core::error::X402Error;
use x402_core::types::{AssetAmount, Network, PaymentRequirements, Price, SupportedKind};
use x402_core::types::SchemeNetworkServer;

use crate::constants::{get_default_asset, DefaultAssetInfo};

/// EVM server implementation for the "exact" payment scheme.
///
/// Mirrors TS: `export class ExactEvmScheme implements SchemeNetworkServer`
pub struct ExactEvmScheme;

impl ExactEvmScheme {
    pub fn new() -> Self {
        Self
    }

    /// Strip currency symbols and whitespace from a money string.
    /// Returns the clean numeric string for string-based arithmetic.
    /// e.g., "$1.50" → "1.50", " 0.001 " → "0.001"
    ///
    /// Mirrors TS: `private parseMoneyToDecimal(money: string | number): number`
    fn parse_money_to_string(money: &str) -> Result<String, X402Error> {
        let clean = money.trim().trim_start_matches('$').trim().to_string();
        // Validate it's a valid number
        clean
            .parse::<f64>()
            .map_err(|_| X402Error::PriceParse(format!("invalid money format: {}", money)))?;
        Ok(clean)
    }

    /// Convert a decimal string amount to an integer token amount.
    ///
    /// Uses string-based arithmetic to avoid f64 precision issues.
    /// e.g., "0.001" with 6 decimals → "1000"
    ///
    /// Mirrors TS: `private convertToTokenAmount(decimalAmount: string, decimals: number): string`
    fn convert_to_token_amount(decimal_amount: &str, decimals: u8) -> Result<String, X402Error> {
        // Work directly on the string to avoid f64 precision loss
        let parts: Vec<&str> = decimal_amount.split('.').collect();
        let int_part = parts[0];
        let dec_part = if parts.len() > 1 { parts[1] } else { "" };

        // Pad or truncate decimal part to match token decimals
        let padded_dec = if dec_part.len() >= decimals as usize {
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
    ///
    /// Mirrors TS: `private defaultMoneyConversion(amount: number, network: Network): AssetAmount`
    fn default_money_conversion(amount: &str, network: &str) -> Result<AssetAmount, X402Error> {
        let asset_info: DefaultAssetInfo = get_default_asset(network).ok_or_else(|| {
            X402Error::UnsupportedNetwork(format!(
                "no default asset configured for network {}",
                network
            ))
        })?;

        let token_amount =
            Self::convert_to_token_amount(amount, asset_info.decimals)?;

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
    ///
    /// Mirrors TS: `async parsePrice(price: Price, network: Network): Promise<AssetAmount>`
    async fn parse_price(&self, price: &Price, network: &Network) -> Result<AssetAmount, X402Error> {
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
            // Parse Money string and convert using string arithmetic (no f64 precision loss)
            Price::Money(money) => {
                let amount_str = Self::parse_money_to_string(money)?;
                Self::default_money_conversion(&amount_str, network)
            }
        }
    }

    /// Enhance payment requirements (passthrough for exact scheme).
    ///
    /// Mirrors TS: `enhancePaymentRequirements()` — returns requirements unchanged.
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

    #[tokio::test]
    async fn test_parse_price_dollar_string() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("$0.001".to_string());
        let result = scheme.parse_price(&price, &"eip155:196".to_string()).await.unwrap();

        assert_eq!(result.amount, "1000");
        assert_eq!(
            result.asset,
            "0x779ded0c9e1022225f8e0630b35a9b54be713736"
        );
    }

    #[tokio::test]
    async fn test_parse_price_plain_number() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("0.01".to_string());
        let result = scheme.parse_price(&price, &"eip155:196".to_string()).await.unwrap();

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
        let result = scheme.parse_price(&price, &"eip155:196".to_string()).await.unwrap();

        assert_eq!(result.amount, "5000");
        assert_eq!(result.asset, "0xCustomToken");
    }

    #[tokio::test]
    async fn test_parse_price_unsupported_network() {
        let scheme = ExactEvmScheme::new();
        let price = Price::Money("$1.00".to_string());
        let result = scheme.parse_price(&price, &"eip155:99999".to_string()).await;

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
}
