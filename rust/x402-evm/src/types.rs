//! EVM-specific payload types.
//!
//! Mirrors: `@x402/mechanisms/evm/src/types.ts`

use serde::{Deserialize, Serialize};

/// Asset transfer methods for the exact EVM scheme.
///
/// Mirrors TS: `export type AssetTransferMethod = "eip3009" | "permit2";`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetTransferMethod {
    /// Uses transferWithAuthorization (USDC, etc.) — recommended for compatible tokens.
    #[serde(rename = "eip3009")]
    Eip3009,
    /// Uses Permit2 + x402Permit2Proxy — universal fallback for any ERC-20.
    Permit2,
}

/// EIP-3009 authorization parameters.
///
/// Mirrors TS: `ExactEIP3009Payload.authorization`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EIP3009Authorization {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
}

/// EIP-3009 payload for tokens with native transferWithAuthorization support.
///
/// Mirrors TS: `export type ExactEIP3009Payload`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEIP3009Payload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub authorization: EIP3009Authorization,
}

/// Permit2 witness data structure.
///
/// Mirrors TS: `export type Permit2Witness`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub to: String,
    pub valid_after: String,
}

/// Permit2 permitted token info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permit2Permitted {
    pub token: String,
    pub amount: String,
}

/// Permit2 authorization parameters.
///
/// Mirrors TS: `export type Permit2Authorization`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub from: String,
    pub permitted: Permit2Permitted,
    pub spender: String,
    pub nonce: String,
    pub deadline: String,
    pub witness: Permit2Witness,
}

/// Permit2 payload for tokens using the Permit2 + x402Permit2Proxy flow.
///
/// Mirrors TS: `export type ExactPermit2Payload`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactPermit2Payload {
    pub signature: String,
    pub permit2_authorization: Permit2Authorization,
}

/// Union of exact EVM payload types (V2).
///
/// Mirrors TS: `export type ExactEvmPayloadV2 = ExactEIP3009Payload | ExactPermit2Payload;`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactEvmPayloadV2 {
    EIP3009(ExactEIP3009Payload),
    Permit2(ExactPermit2Payload),
}

impl ExactEvmPayloadV2 {
    /// Check if this is a Permit2 payload.
    ///
    /// Mirrors TS: `isPermit2Payload()`
    pub fn is_permit2(&self) -> bool {
        matches!(self, Self::Permit2(_))
    }

    /// Check if this is an EIP-3009 payload.
    ///
    /// Mirrors TS: `isEIP3009Payload()`
    pub fn is_eip3009(&self) -> bool {
        matches!(self, Self::EIP3009(_))
    }
}
