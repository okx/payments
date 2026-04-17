//! Chain and asset configuration constants.
//!
//! Mirrors: `@x402/mechanisms/evm/src/shared/defaultAssets.ts`
//! Extended with X Layer asset pre-registration.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Base stablecoin asset configuration.
///
/// Mirrors TS: `DefaultAssetInfo`
#[derive(Debug, Clone)]
pub struct DefaultAssetInfo {
    /// Token contract address.
    pub address: &'static str,
    /// EIP-712 domain name (must match the token's domain separator).
    pub name: &'static str,
    /// EIP-712 domain version (must match the token's domain separator).
    pub version: &'static str,
    /// Token decimal places.
    pub decimals: u8,
    /// Transfer method override: "permit2" for tokens without EIP-3009.
    pub asset_transfer_method: Option<&'static str>,
    /// Whether the token supports EIP-2612 permit().
    pub supports_eip2612: bool,
}

/// Chain configuration.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// CAIP-2 network identifier.
    pub network: &'static str,
    /// EVM chain ID.
    pub chain_id: u64,
}

// ---------------------------------------------------------------------------
// OKX X Layer assets
// ---------------------------------------------------------------------------

/// X Layer mainnet chain config.
pub const XLAYER_MAINNET: ChainConfig = ChainConfig {
    network: "eip155:196",
    chain_id: 196,
};

/// X Layer mainnet USDT
/// EIP-712 name uses Unicode ₮ (U+20AE): "USD₮0"
pub const XLAYER_MAINNET_USDT: DefaultAssetInfo = DefaultAssetInfo {
    address: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
    name: "USD\u{20AE}0",
    version: "1",
    decimals: 6,
    asset_transfer_method: None,
    supports_eip2612: false,
};

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Default stablecoins map for OKX X Layer (initialized once).
///
/// Mirrors TS: `DEFAULT_STABLECOINS` record (OKX subset only).
static DEFAULT_STABLECOINS: LazyLock<HashMap<&'static str, DefaultAssetInfo>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        map.insert("eip155:196", XLAYER_MAINNET_USDT);
        map
    });

/// Look up the default stablecoin for a network.
///
/// Mirrors TS: `getDefaultAsset(network: Network): ExactDefaultAssetInfo`
pub fn get_default_asset(network: &str) -> Option<DefaultAssetInfo> {
    DEFAULT_STABLECOINS.get(network).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xlayer_mainnet_config() {
        let asset = get_default_asset("eip155:196").unwrap();
        assert_eq!(asset.address, "0x779ded0c9e1022225f8e0630b35a9b54be713736");
        assert_eq!(asset.decimals, 6);
        assert_eq!(asset.name, "USD\u{20AE}0");
        assert!(asset.name.contains('\u{20AE}')); // ₮
    }

    #[test]
    fn test_unknown_network() {
        assert!(get_default_asset("eip155:99999").is_none());
    }

    #[test]
    fn test_xlayer_chain_ids() {
        assert_eq!(XLAYER_MAINNET.chain_id, 196);
        assert_eq!(XLAYER_MAINNET.network, "eip155:196");
    }
}
