//! Facilitator request/response types and error definitions.
//!
//! Mirrors: `@x402/core/src/types/facilitator.ts`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{Network, PaymentPayload, PaymentRequirements};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Request body for `POST /verify`.
///
/// Mirrors TS: `export type VerifyRequest`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

/// Response body from `POST /verify`.
///
/// Mirrors TS: `export type VerifyResponse`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

/// Request body for `POST /settle`.
///
/// Mirrors TS: `export type SettleRequest`
/// OKX extension: `sync_settle` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleRequest {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
    /// OKX extension: if true, wait for on-chain confirmation (exact scheme only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_settle: Option<bool>,
}

/// Response body from `POST /settle`.
///
/// Mirrors TS: `export type SettleResponse`
/// OKX extensions: `status` field, `transaction` (renamed from Coinbase's `txHash`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    pub transaction: String,
    pub network: Network,
    /// Actual amount settled in atomic token units.
    /// Present for schemes like `upto` where settlement amount may differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    /// OKX extension: "pending" | "success" | "timeout".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, serde_json::Value>>,
}

/// A single supported scheme/network/version combination.
///
/// Mirrors TS: `export type SupportedKind`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedKind {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    pub scheme: String,
    pub network: Network,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<HashMap<String, serde_json::Value>>,
}

/// Response body from `GET /supported`.
///
/// Mirrors TS: `export type SupportedResponse`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedResponse {
    pub kinds: Vec<SupportedKind>,
    pub extensions: Vec<String>,
    /// CAIP family pattern → signer addresses.
    pub signers: HashMap<String, Vec<String>>,
}

/// Response body from `GET /settle/status?txHash=...`.
///
/// OKX extension: query on-chain settlement status by transaction hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleStatusResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<Network>,
    /// "pending" | "success" | "failed"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Error thrown when payment verification fails.
///
/// Mirrors TS: `export class VerifyError extends Error`
#[derive(Debug, Clone, thiserror::Error)]
#[error("{}", self.display_message())]
pub struct VerifyError {
    pub status_code: u16,
    pub invalid_reason: Option<String>,
    pub invalid_message: Option<String>,
    pub payer: Option<String>,
}

impl VerifyError {
    pub fn new(status_code: u16, response: &VerifyResponse) -> Self {
        Self {
            status_code,
            invalid_reason: response.invalid_reason.clone(),
            invalid_message: response.invalid_message.clone(),
            payer: response.payer.clone(),
        }
    }

    fn display_message(&self) -> String {
        let reason = self.invalid_reason.as_deref().unwrap_or("unknown reason");
        match &self.invalid_message {
            Some(msg) => format!("{}: {}", reason, msg),
            None => reason.to_string(),
        }
    }
}

/// Error thrown when payment settlement fails.
///
/// Mirrors TS: `export class SettleError extends Error`
#[derive(Debug, Clone, thiserror::Error)]
#[error("{}", self.display_message())]
pub struct SettleError {
    pub status_code: u16,
    pub error_reason: Option<String>,
    pub error_message: Option<String>,
    pub payer: Option<String>,
    pub transaction: String,
    pub network: Network,
}

impl SettleError {
    pub fn new(status_code: u16, response: &SettleResponse) -> Self {
        Self {
            status_code,
            error_reason: response.error_reason.clone(),
            error_message: response.error_message.clone(),
            payer: response.payer.clone(),
            transaction: response.transaction.clone(),
            network: response.network.clone(),
        }
    }

    fn display_message(&self) -> String {
        let reason = self.error_reason.as_deref().unwrap_or("unknown reason");
        match &self.error_message {
            Some(msg) => format!("{}: {}", reason, msg),
            None => reason.to_string(),
        }
    }
}

/// Error thrown when a facilitator returns malformed success payload data.
///
/// Mirrors TS: `export class FacilitatorResponseError extends Error`
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct FacilitatorResponseError(pub String);
