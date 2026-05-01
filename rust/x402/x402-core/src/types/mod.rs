//! Core type definitions for the x402 payment protocol.
//!
//! Mirrors: `@x402/core/src/types/index.ts`

mod extensions;
mod facilitator;
mod mechanisms;
mod payments;

pub use extensions::*;
pub use facilitator::*;
pub use mechanisms::*;
pub use payments::*;

/// Network identifier in CAIP-2 format (e.g., "eip155:196").
///
/// Mirrors TS: `export type Network = \`${string}:${string}\`;`
pub type Network = String;

/// User-friendly money amount (e.g., "$0.10", "0.10").
///
/// Mirrors TS: `export type Money = string | number;`
pub type Money = String;

/// Resolved asset amount in atomic token units.
///
/// Mirrors TS: `export type AssetAmount`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetAmount {
    pub asset: String,
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// Price can be either a simple money string or a fully resolved AssetAmount.
///
/// Mirrors TS: `export type Price = Money | AssetAmount;`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Price {
    Money(Money),
    Asset(AssetAmount),
}
