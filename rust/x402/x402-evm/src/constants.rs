//! Chain and asset configuration constants.
//! Extended with X Layer asset pre-registration.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Base stablecoin asset configuration.
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
// Permit2 — canonical addresses are CREATE2-vanity, identical on every EVM
// chain. Any change here breaks signature verification system-wide.
// ---------------------------------------------------------------------------

pub const PERMIT2_ADDRESS: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";

pub const X402_EXACT_PERMIT2_PROXY_ADDRESS: &str =
    "0x402085c248EeA27D92E8b30b2C58ed07f9E20001";

pub const PERMIT2_EIP712_DOMAIN_NAME: &str = "Permit2";

/// Field order is canonical and ABI-significant — any reorder breaks
/// onchain signature verification.
pub const PERMIT2_EXACT_WITNESS_TYPE_STRING: &str =
    "PermitWitnessTransferFrom(\
TokenPermissions permitted,\
address spender,\
uint256 nonce,\
uint256 deadline,\
Witness witness)\
TokenPermissions(address token,uint256 amount)\
Witness(address to,uint256 validAfter)";

pub const X402_UPTO_PERMIT2_PROXY_ADDRESS: &str =
    "0x4020e7393B728A3939659E5732F87fdd8e680002";

/// Witness adds an `address facilitator` field between `to` and `validAfter`
/// — this changes the typehash, so exact and upto signatures are not
/// cross-compatible.
pub const PERMIT2_UPTO_WITNESS_TYPE_STRING: &str =
    "PermitWitnessTransferFrom(\
TokenPermissions permitted,\
address spender,\
uint256 nonce,\
uint256 deadline,\
Witness witness)\
TokenPermissions(address token,uint256 amount)\
Witness(address to,address facilitator,uint256 validAfter)";

// ---------------------------------------------------------------------------
// OKX X Layer assets
// ---------------------------------------------------------------------------

/// X Layer mainnet chain config.
pub const XLAYER_MAINNET: ChainConfig = ChainConfig {
    network: "eip155:196",
    chain_id: 196,
};

/// EIP-712 name uses Unicode ₮ (U+20AE): "USD₮0".
pub const XLAYER_MAINNET_USDT: DefaultAssetInfo = DefaultAssetInfo {
    address: "0x779ded0c9e1022225f8e0630b35a9b54be713736",
    name: "USD\u{20AE}0",
    version: "1",
    decimals: 6,
    asset_transfer_method: None,
    supports_eip2612: false,
};

pub const XLAYER_TESTNET: ChainConfig = ChainConfig {
    network: "eip155:1952",
    chain_id: 1952,
};

pub const XLAYER_TESTNET_USDT: DefaultAssetInfo = DefaultAssetInfo {
    address: "0x9e29b3aada05bf2d2c827af80bd28dc0b9b4fb0c",
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
static DEFAULT_STABLECOINS: LazyLock<HashMap<&'static str, DefaultAssetInfo>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        map.insert("eip155:196", XLAYER_MAINNET_USDT);
        map.insert("eip155:1952", XLAYER_TESTNET_USDT);
        map
    });

/// Look up the default stablecoin for a network.
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

    #[test]
    fn test_permit2_canonical_address() {
        // Canonical Uniswap Permit2 address — same on every EVM chain.
        // Any change here breaks signature verification across the entire system.
        assert_eq!(PERMIT2_ADDRESS, "0x000000000022D473030F116dDEE9F6B43aC78BA3");
    }

    #[test]
    fn test_x402_exact_permit2_proxy_address() {
        // Exact-scheme proxy address — ends in `0001` (vanity tag for exact).
        assert_eq!(
            X402_EXACT_PERMIT2_PROXY_ADDRESS,
            "0x402085c248EeA27D92E8b30b2C58ed07f9E20001"
        );
    }

    #[test]
    fn test_permit2_exact_witness_type_string_field_order() {
        // Field order in the typeString determines the typehash, which is
        // baked into the EIP-712 signature. The order must be:
        // 1) Outer struct PermitWitnessTransferFrom (alphabetical by Solidity rule)
        // 2) Referenced structs sorted by name: TokenPermissions < Witness
        let s = PERMIT2_EXACT_WITNESS_TYPE_STRING;

        let outer = s.find("PermitWitnessTransferFrom(").expect("outer struct");
        let token_permissions = s.find("TokenPermissions(address token").expect("token perms");
        let witness = s.find("Witness(address to").expect("witness");

        assert!(outer < token_permissions, "outer struct must come first");
        assert!(
            token_permissions < witness,
            "TokenPermissions < Witness (alphabetical referenced struct order)"
        );
    }

    #[test]
    fn test_permit2_eip712_domain_name() {
        assert_eq!(PERMIT2_EIP712_DOMAIN_NAME, "Permit2");
    }

    #[test]
    fn test_x402_upto_permit2_proxy_address() {
        // Upto proxy is a separate deployment from the exact proxy — same
        // CREATE2 deterministic deploy across chains, but the settle ABI
        // takes one extra `uint256 settlementAmount` argument.
        assert_eq!(
            X402_UPTO_PERMIT2_PROXY_ADDRESS,
            "0x4020e7393B728A3939659E5732F87fdd8e680002"
        );
    }

    #[test]
    fn test_permit2_upto_witness_type_string_has_facilitator() {
        // The only structural difference between exact and upto type
        // strings is the Witness struct: upto inserts `address facilitator`
        // between `to` and `validAfter`. Both that field's presence and
        // its position are typehash-affecting — pin them.
        let s = PERMIT2_UPTO_WITNESS_TYPE_STRING;
        let witness = s
            .find("Witness(address to,address facilitator,uint256 validAfter)")
            .expect("upto Witness must list facilitator between to and validAfter");
        let outer = s.find("PermitWitnessTransferFrom(").expect("outer struct");
        let token_permissions =
            s.find("TokenPermissions(address token,uint256 amount)").expect("token perms");
        assert!(outer < token_permissions, "outer struct first");
        assert!(token_permissions < witness, "TokenPermissions < Witness order");
    }

    #[test]
    fn test_upto_typestring_differs_from_exact() {
        // Sanity: the two type strings must differ — otherwise we'd be
        // claiming exact and upto are the same scheme, which they're not.
        assert_ne!(
            PERMIT2_EXACT_WITNESS_TYPE_STRING,
            PERMIT2_UPTO_WITNESS_TYPE_STRING
        );
    }

    #[test]
    fn test_xlayer_testnet_config() {
        let asset = get_default_asset("eip155:1952").unwrap();
        assert_eq!(asset.address, "0x9e29b3aada05bf2d2c827af80bd28dc0b9b4fb0c");
        assert_eq!(asset.decimals, 6);
        assert_eq!(asset.name, "USD\u{20AE}0");
        assert_eq!(asset.version, "1");
        assert!(asset.name.contains('\u{20AE}'));
        assert!(asset.asset_transfer_method.is_none());
    }

    #[test]
    fn test_xlayer_testnet_chain_ids() {
        assert_eq!(XLAYER_TESTNET.chain_id, 1952);
        assert_eq!(XLAYER_TESTNET.network, "eip155:1952");
    }

    #[test]
    fn test_mainnet_and_testnet_addresses_differ() {
        let mainnet = get_default_asset("eip155:196").unwrap();
        let testnet = get_default_asset("eip155:1952").unwrap();
        assert_ne!(mainnet.address, testnet.address);
    }
}
