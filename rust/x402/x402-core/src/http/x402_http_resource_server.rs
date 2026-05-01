//! HTTP-layer resource server handling payment headers and 402 responses.
//!
//! Mirrors: `@x402/core/src/http/x402HTTPResourceServer.ts`
//! and `@x402/core/src/http/index.ts` (header encode/decode functions)

use std::collections::HashMap;
use std::time::Duration;

use crate::error::X402Error;
use crate::types::{
    PaymentPayload, PaymentRequired, PaymentRequirements, SettleResponse, VerifyResponse,
};
use crate::utils::{safe_base64_decode, safe_base64_encode};

/// Default poll interval for settle/status queries.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Default poll deadline for settle/status queries.
pub const DEFAULT_POLL_DEADLINE: Duration = Duration::from_secs(5);

/// Result of polling `GET /settle/status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollResult {
    /// status="success" — transaction confirmed on-chain.
    Success,
    /// success=false — transaction failed on-chain.
    Failed,
    /// Poll deadline expired while status was still "pending".
    Timeout,
}

/// HTTP header name for the payment signature (client → server).
pub const PAYMENT_SIGNATURE_HEADER: &str = "PAYMENT-SIGNATURE";

/// HTTP header name for the payment required response (server → client).
pub const PAYMENT_REQUIRED_HEADER: &str = "PAYMENT-REQUIRED";

/// HTTP header name for the payment response (server → client after settlement).
pub const PAYMENT_RESPONSE_HEADER: &str = "PAYMENT-RESPONSE";

/// HTTP header name for settlement overrides (handler → middleware, internal only).
///
/// Mirrors TS: `SETTLEMENT_OVERRIDES_HEADER` from `x402HTTPResourceServer.ts`
pub const SETTLEMENT_OVERRIDES_HEADER: &str = "settlement-overrides";

/// Settlement overrides for partial settlement (e.g., upto scheme billing by actual usage).
///
/// Mirrors TS: `export interface SettlementOverrides`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementOverrides {
    /// Amount to settle. Supports three formats:
    /// - Raw atomic units: `"1000"`
    /// - Percent of PaymentRequirements.amount: `"50%"`
    /// - Dollar price: `"$0.05"` (uses requirements.extra.decimals or defaults to 6)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
}

/// Resolve a settlement override amount string to a final atomic-unit string.
///
/// Supports three formats (mirrors TS `resolveSettlementOverrideAmount`):
/// - Raw atomic units: `"1000"`
/// - Percent: `"50%"` or `"33.33%"` (up to 2 decimal places, floored)
/// - Dollar price: `"$0.05"` (uses requirements.extra.decimals or defaults to 6)
pub fn resolve_settlement_override_amount(
    raw_amount: &str,
    requirements: &crate::types::PaymentRequirements,
) -> Result<String, crate::error::X402Error> {
    // Percent format: "50%" or "33.33%"
    if let Some(pct) = raw_amount.strip_suffix('%') {
        let parts: Vec<&str> = pct.split('.').collect();
        let int_part: u128 = parts[0].parse().map_err(|_| {
            crate::error::X402Error::PriceParse(format!("invalid percent: {}", raw_amount))
        })?;
        let dec_part: u128 = if parts.len() > 1 {
            let d = format!("{:0<2}", &parts[1][..parts[1].len().min(2)]);
            d.parse().unwrap_or(0)
        } else {
            0
        };
        let scaled_percent = int_part * 100 + dec_part;
        let base: u128 = requirements.amount.parse().map_err(|_| {
            crate::error::X402Error::PriceParse(format!(
                "invalid base amount: {}",
                requirements.amount
            ))
        })?;
        return Ok(((base * scaled_percent) / 10000).to_string());
    }

    // Dollar price format: "$0.05"
    if let Some(dollars_str) = raw_amount.strip_prefix('$') {
        let decimals: u32 = requirements
            .extra
            .get("decimals")
            .and_then(|v| v.as_u64())
            .unwrap_or(6) as u32;
        let dollars: f64 = dollars_str.parse().map_err(|_| {
            crate::error::X402Error::PriceParse(format!("invalid dollar amount: {}", raw_amount))
        })?;
        let amount = (dollars * 10f64.powi(decimals as i32)).round() as u128;
        return Ok(amount.to_string());
    }

    // Raw atomic units
    Ok(raw_amount.to_string())
}

// ---------------------------------------------------------------------------
// Header encoding/decoding (mirrors core/src/http/index.ts)
// ---------------------------------------------------------------------------

/// Encode a PaymentPayload as a base64 header value.
///
/// Mirrors TS: `encodePaymentSignatureHeader`
pub fn encode_payment_signature_header(payload: &PaymentPayload) -> Result<String, X402Error> {
    let json = serde_json::to_string(payload)?;
    Ok(safe_base64_encode(&json))
}

/// Decode a base64 PAYMENT-SIGNATURE header into a PaymentPayload.
///
/// Mirrors TS: `decodePaymentSignatureHeader`
pub fn decode_payment_signature_header(header: &str) -> Result<PaymentPayload, X402Error> {
    let json = safe_base64_decode(header)?;
    let payload: PaymentPayload = serde_json::from_str(&json)?;
    Ok(payload)
}

/// Encode a PaymentRequired as a base64 header value.
///
/// Mirrors TS: `encodePaymentRequiredHeader`
pub fn encode_payment_required_header(required: &PaymentRequired) -> Result<String, X402Error> {
    let json = serde_json::to_string(required)?;
    Ok(safe_base64_encode(&json))
}

/// Decode a base64 PAYMENT-REQUIRED header into a PaymentRequired.
///
/// Mirrors TS: `decodePaymentRequiredHeader`
pub fn decode_payment_required_header(header: &str) -> Result<PaymentRequired, X402Error> {
    let json = safe_base64_decode(header)?;
    let required: PaymentRequired = serde_json::from_str(&json)?;
    Ok(required)
}

/// Encode a SettleResponse as a base64 header value.
///
/// Mirrors TS: `encodePaymentResponseHeader`
pub fn encode_payment_response_header(response: &SettleResponse) -> Result<String, X402Error> {
    let json = serde_json::to_string(response)?;
    Ok(safe_base64_encode(&json))
}

/// Decode a base64 PAYMENT-RESPONSE header into a SettleResponse.
///
/// Mirrors TS: `decodePaymentResponseHeader`
pub fn decode_payment_response_header(header: &str) -> Result<SettleResponse, X402Error> {
    let json = safe_base64_decode(header)?;
    let response: SettleResponse = serde_json::from_str(&json)?;
    Ok(response)
}

// ---------------------------------------------------------------------------
// Route configuration types (mirrors x402HTTPResourceServer route config)
// ---------------------------------------------------------------------------

/// HTTP request context passed to dynamic pricing resolver.
///
/// Mirrors TS: `HTTPRequestContext` fields used in DynamicPrice/DynamicPayTo.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

/// Resolved pricing for a single accept config after dynamic resolution.
#[derive(Debug, Clone)]
pub struct ResolvedAccept {
    pub scheme: String,
    pub price: String,
    pub network: String,
    pub pay_to: String,
    pub max_timeout_seconds: Option<u64>,
    pub extra: Option<HashMap<String, serde_json::Value>>,
}

/// Dynamic resolver that can override price/payTo per-request.
///
/// Receives the request context and the original `AcceptConfig`, returns
/// a `ResolvedAccept` with possibly overridden price/pay_to.
///
/// Mirrors TS: `DynamicPrice` / `DynamicPayTo` but as a single hook.
pub type PaymentResolverFn = Box<
    dyn Fn(
            &RequestContext,
            &AcceptConfig,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ResolvedAccept> + Send>>
        + Send
        + Sync,
>;

/// Configuration for a single payment option within a route.
///
/// Mirrors TS: `PaymentOption` from `x402HTTPResourceServer.ts`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptConfig {
    pub scheme: String,
    pub price: String,
    pub network: String,
    pub pay_to: String,
    /// Maximum timeout in seconds for payment processing.
    /// Defaults to 300 (5 minutes), aligning with TS `PaymentOption.maxTimeoutSeconds`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_timeout_seconds: Option<u64>,
    /// Extra scheme-specific metadata (e.g., user-provided extra merged into requirements).
    /// Mirrors TS `PaymentOption.extra`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<HashMap<String, serde_json::Value>>,
}

/// Configuration for a payment-protected route.
///
/// Mirrors TS: `RouteConfig` from `x402HTTPResourceServer.ts`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePaymentConfig {
    pub accepts: Vec<AcceptConfig>,
    pub description: String,
    pub mime_type: String,
    /// OKX extension: if true, settle will wait for on-chain confirmation.
    /// Default: false (async settle, returns status="pending").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_settle: Option<bool>,
}

/// Result of the settlement timeout hook.
pub struct SettlementTimeoutResult {
    pub confirmed: bool,
}

/// Hook called when the facilitator returns `status="timeout"`.
/// The hook should verify the transaction on-chain and return whether it was confirmed.
/// If confirmed=true, the resource is delivered (200); otherwise 402 is returned.
///
/// Mirrors TS: `OnSettlementTimeoutHook` from `x402HTTPResourceServer.ts`
///
/// # Arguments
/// - `tx_hash` - The transaction hash to verify on-chain
/// - `network` - The CAIP-2 network identifier (e.g., "eip155:196")
pub type OnSettlementTimeoutHook = Box<
    dyn Fn(
            String,
            String,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = SettlementTimeoutResult> + Send>>
        + Send
        + Sync,
>;

/// Routes configuration mapping "METHOD /path" → payment config.
/// Uses HashMap to align with TS plain-object style.
///
/// Mirrors TS: `RoutesConfig` from `x402HTTPResourceServer.ts`
pub type RoutesConfig = HashMap<String, RoutePaymentConfig>;

// ---------------------------------------------------------------------------
// Server Hooks — Mirrors TS lifecycle hooks from x402ResourceServer
// ---------------------------------------------------------------------------

/// Result of the `onProtectedRequest` hook.
///
/// Mirrors TS: `OnProtectedRequestHookResult`
pub struct ProtectedRequestResult {
    /// If true, bypass payment and grant free access (e.g., VIP, OAuth).
    pub grant_access: bool,
    /// If true, abort the request entirely (e.g., IP ban, rate limit).
    pub abort: bool,
    /// Reason for abort (returned to client).
    pub reason: Option<String>,
}

/// Result of the `onBeforeVerify` / `onBeforeSettle` hooks.
///
/// Mirrors TS: `BeforeVerifyHookResult` / `BeforeSettleHookResult`
pub struct BeforeHookResult {
    /// If true, abort the operation.
    pub abort: bool,
    /// Reason for abort.
    pub reason: Option<String>,
}

/// Result of the `onVerifyFailure` hook.
///
/// Mirrors TS: `OnVerifyFailureHookResult`
pub struct VerifyRecoveryResult {
    /// If true, override the failure and proceed with settlement.
    pub recovered: bool,
    /// Recovered verify response (required when recovered=true).
    pub result: Option<VerifyResponse>,
}

/// Result of the `onSettleFailure` hook.
///
/// Mirrors TS: `OnSettleFailureHookResult`
pub struct SettleRecoveryResult {
    /// If true, override the failure and deliver the resource.
    pub recovered: bool,
    /// Recovered settle response (required when recovered=true).
    pub result: Option<SettleResponse>,
}

/// Context passed to verify hooks.
#[derive(Debug, Clone)]
pub struct VerifyContext {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

/// Context passed to `onAfterVerify` hook.
#[derive(Debug, Clone)]
pub struct VerifyResultContext {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
    pub verify_response: VerifyResponse,
}

/// Context passed to settle hooks.
#[derive(Debug, Clone)]
pub struct SettleContext {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

/// Context passed to `onAfterSettle` hook.
#[derive(Debug, Clone)]
pub struct SettleResultContext {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
    pub settle_response: SettleResponse,
}

/// Hook: called when a protected route is accessed, before payment verification.
/// Can grant free access, abort, or proceed to payment flow.
///
/// Mirrors TS: `onProtectedRequest` from `x402HTTPResourceServer.ts`
pub type OnProtectedRequestHook = Box<
    dyn Fn(
            RequestContext,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = ProtectedRequestResult> + Send>>
        + Send
        + Sync,
>;

/// Hook: called before payment verification.
/// Can abort the verification process.
///
/// Mirrors TS: `onBeforeVerify` from `x402ResourceServer.ts`
pub type OnBeforeVerifyHook = Box<
    dyn Fn(
            VerifyContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = BeforeHookResult> + Send>>
        + Send
        + Sync,
>;

/// Hook: called after successful payment verification (side-effect only).
///
/// Mirrors TS: `onAfterVerify` from `x402ResourceServer.ts`
pub type OnAfterVerifyHook = Box<
    dyn Fn(VerifyResultContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Hook: called when payment verification fails.
/// Can recover from the failure by providing a valid VerifyResponse.
///
/// Mirrors TS: `onVerifyFailure` from `x402ResourceServer.ts`
pub type OnVerifyFailureHook = Box<
    dyn Fn(
            VerifyContext,
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<VerifyRecoveryResult>> + Send>,
        > + Send
        + Sync,
>;

/// Hook: called before payment settlement.
/// Can abort the settlement process.
///
/// Mirrors TS: `onBeforeSettle` from `x402ResourceServer.ts`
pub type OnBeforeSettleHook = Box<
    dyn Fn(
            SettleContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = BeforeHookResult> + Send>>
        + Send
        + Sync,
>;

/// Hook: called after successful payment settlement (side-effect only).
///
/// Mirrors TS: `onAfterSettle` from `x402ResourceServer.ts`
pub type OnAfterSettleHook = Box<
    dyn Fn(SettleResultContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// Hook: called when payment settlement fails.
/// Can recover from the failure by providing a valid SettleResponse.
///
/// Mirrors TS: `onSettleFailure` from `x402ResourceServer.ts`
pub type OnSettleFailureHook = Box<
    dyn Fn(
            SettleContext,
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<SettleRecoveryResult>> + Send>,
        > + Send
        + Sync,
>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PaymentRequirements, ResourceInfo};

    #[test]
    fn test_payment_required_header_round_trip() {
        let required = PaymentRequired {
            x402_version: 2,
            error: None,
            resource: ResourceInfo {
                url: "https://example.com/api".to_string(),
                description: Some("Test API".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            accepts: vec![PaymentRequirements {
                scheme: "exact".to_string(),
                network: "eip155:196".to_string(),
                asset: "0x779ded0c9e1022225f8e0630b35a9b54be713736".to_string(),
                amount: "1000".to_string(),
                pay_to: "0xSeller".to_string(),
                max_timeout_seconds: 60,
                extra: HashMap::new(),
            }],
            extensions: None,
        };

        let encoded = encode_payment_required_header(&required).unwrap();
        let decoded = decode_payment_required_header(&encoded).unwrap();
        assert_eq!(decoded.x402_version, 2);
        assert_eq!(decoded.accepts[0].scheme, "exact");
        assert_eq!(decoded.accepts[0].network, "eip155:196");
    }

    #[test]
    fn test_payment_payload_header_round_trip() {
        let payload = PaymentPayload {
            x402_version: 2,
            resource: None,
            accepted: PaymentRequirements {
                scheme: "exact".to_string(),
                network: "eip155:196".to_string(),
                asset: "0xToken".to_string(),
                amount: "1000".to_string(),
                pay_to: "0xSeller".to_string(),
                max_timeout_seconds: 60,
                extra: HashMap::new(),
            },
            payload: {
                let mut m = HashMap::new();
                m.insert(
                    "signature".to_string(),
                    serde_json::Value::String("0xabc".to_string()),
                );
                m
            },
            extensions: None,
        };

        let encoded = encode_payment_signature_header(&payload).unwrap();
        let decoded = decode_payment_signature_header(&encoded).unwrap();
        assert_eq!(decoded.x402_version, 2);
        assert_eq!(decoded.accepted.scheme, "exact");
    }
}
