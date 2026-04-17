//! Payment-related types for the x402 protocol.
//!
//! Mirrors: `@x402/core/src/types/payments.ts`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Network;

/// Description of the protected resource.
///
/// Mirrors TS: `export interface ResourceInfo`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Payment requirements for a specific scheme/network combination.
/// Used both in 402 response `accepts` array and in `paymentPayload.accepted`.
///
/// Mirrors TS: `export type PaymentRequirements`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: Network,
    pub asset: String,
    pub amount: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// The 402 Payment Required response body sent to clients.
///
/// Mirrors TS: `export type PaymentRequired`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource: ResourceInfo,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

/// Payment payload submitted by the client (buyer) via PAYMENT-SIGNATURE header.
///
/// Mirrors TS: `export type PaymentPayload`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceInfo>,
    pub accepted: PaymentRequirements,
    pub payload: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}
