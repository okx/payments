//! Data types for MPP EVM SDK.
//!
//! Aligned with OKX Payments SDK 二期 MPP 集成方案 §8 and [Pay] MPP EVM API 方案.
//! Covers: challenge method details, credential payloads, receipts, channel state,
//! and EIP-712 voucher domain constants.

use serde::{Deserialize, Serialize};

// ==================== SA API Response Wrapper ====================

/// SA API unified response: `{ code, data, msg }`.
#[derive(Debug, Clone, Deserialize)]
pub struct SaApiResponse<T> {
    pub code: u32,
    pub data: Option<T>,
    #[serde(default)]
    pub msg: String,
}

// ==================== EIP-712 Voucher Domain (spec §8.4) ====================

/// EIP-712 domain name used for Voucher signatures in OKX MPP.
pub const VOUCHER_DOMAIN_NAME: &str = "EVM Payment Channel";

/// EIP-712 domain version for Voucher signatures.
pub const VOUCHER_DOMAIN_VERSION: &str = "1";

/// Default X Layer chain ID.
pub const DEFAULT_CHAIN_ID: u64 = 196;

/// EIP-712 Voucher typed data (client-side signing reference).
///
/// Struct: `Voucher { bytes32 channelId; uint128 cumulativeAmount; }`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voucher {
    pub channel_id: String,
    pub cumulative_amount: String,
}

/// EIP-712 domain separator for Voucher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherDomain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: String,
}

impl VoucherDomain {
    pub fn new(chain_id: u64, escrow_contract: impl Into<String>) -> Self {
        Self {
            name: VOUCHER_DOMAIN_NAME.to_string(),
            version: VOUCHER_DOMAIN_VERSION.to_string(),
            chain_id,
            verifying_contract: escrow_contract.into(),
        }
    }
}

// ==================== Challenge methodDetails (spec §8.1) ====================

/// EVM-specific method details for a Charge challenge.
///
/// Placed inside `ChargeRequest.methodDetails` (base64url-encoded within
/// the challenge request field).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeMethodDetails {
    pub chain_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permit2_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splits: Option<Vec<ChargeSplit>>,
}

/// Charge split (fixed amount).
///
/// Constraints: `sum(splits[].amount) < request.amount`;
/// primary recipient must retain non-zero balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChargeSplit {
    pub amount: String,
    pub recipient: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

/// EVM-specific method details for a Session challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMethodDetails {
    pub chain_id: u64,
    pub escrow_contract: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_voucher_delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splits: Option<Vec<SessionSplit>>,
}

/// Session split (proportional, basis points).
///
/// Constraints: `bps` in `[1, 9999]`; `sum(splits[].bps) < 10000`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSplit {
    pub recipient: String,
    pub bps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

// ==================== Credential Payload (spec §8.2 / §8.3) ====================

/// EIP-3009 `transferWithAuthorization` authorization object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eip3009Authorization {
    /// Always `"eip-3009"`.
    #[serde(rename = "type")]
    pub auth_type: String,
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub splits: Option<Vec<Eip3009Split>>,
}

impl Eip3009Authorization {
    /// EIP-3009 auth type constant.
    pub const TYPE: &'static str = "eip-3009";
}

/// Independent split signature for Charge payment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eip3009Split {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
    pub signature: String,
}

// ==================== Receipts (spec §8.4) ====================

/// Receipt returned by POST `/charge/settle` and `/charge/verifyHash`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeReceipt {
    pub method: String,
    pub reference: String,
    pub status: String,
    pub timestamp: String,
    pub chain_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

/// Receipt returned by POST `/session/{open,voucher,topUp,settle,close}`.
///
/// Per the API 方案, `spent` is omitted on open/voucher responses and present on
/// topUp/settle/close — modeled as `Option` to handle both shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReceipt {
    pub method: String,
    pub intent: String,
    pub status: String,
    pub timestamp: String,
    pub chain_id: u64,
    pub challenge_id: String,
    pub channel_id: String,
    pub accepted_cumulative: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<u64>,
}

// ==================== Channel Status (spec §8.4) ====================

/// Response from GET `/session/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatus {
    pub channel_id: String,
    pub payer: String,
    pub payee: String,
    pub token: String,
    pub deposit: String,
    pub cumulative_amount: String,
    pub settled_on_chain: String,
    pub session_status: String,
    pub remaining_balance: String,
}

// ==================== Server Accounting State (spec §8.4) ====================

/// Server-side per-session accounting, maintained locally by the Seller SDK.
///
/// Invariants:
/// - `accepted_cumulative` is monotonically non-decreasing (matches highest
///   SA-accepted voucher).
/// - `spent` is monotonically non-decreasing (matches total amount consumed).
/// - `available = accepted_cumulative - spent`.
/// - `remaining_balance = deposit - accepted_cumulative` (deposit from channel).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAccountingState {
    pub accepted_cumulative: u128,
    pub spent: u128,
    pub settled_on_chain: u128,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_receipt_minimal_round_trip() {
        let json = r#"{
            "method":"evm",
            "reference":"0xabc",
            "status":"success",
            "timestamp":"2026-04-01T12:00:00Z",
            "chainId":196
        }"#;
        let r: ChargeReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(r.method, "evm");
        assert_eq!(r.chain_id, 196);
        assert!(r.confirmations.is_none());
    }

    #[test]
    fn session_receipt_open_shape_no_spent() {
        // Per [Pay] MPP EVM API 方案: open response has no "spent" field.
        let json = r#"{
            "method":"evm",
            "intent":"session",
            "status":"success",
            "timestamp":"2026-04-01T12:00:00Z",
            "chainId":196,
            "challengeId":"ch-1",
            "channelId":"0xdead",
            "acceptedCumulative":"0"
        }"#;
        let r: SessionReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(r.channel_id, "0xdead");
        assert!(r.spent.is_none());
    }

    #[test]
    fn session_receipt_close_shape_with_spent() {
        let json = r#"{
            "method":"evm",
            "intent":"session",
            "status":"success",
            "timestamp":"2026-04-01T12:00:00Z",
            "chainId":196,
            "challengeId":"ch-1",
            "channelId":"0xdead",
            "acceptedCumulative":"1000",
            "spent":"1000",
            "reference":"0xclosetx"
        }"#;
        let r: SessionReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(r.spent.as_deref(), Some("1000"));
        assert_eq!(r.reference.as_deref(), Some("0xclosetx"));
    }

    #[test]
    fn charge_method_details_with_splits_serializes_camel_case() {
        let d = ChargeMethodDetails {
            chain_id: 196,
            fee_payer: Some(true),
            permit2_address: None,
            memo: None,
            splits: Some(vec![ChargeSplit {
                amount: "100".into(),
                recipient: "0xabc".into(),
                memo: Some("fee".into()),
            }]),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["chainId"], 196);
        assert_eq!(json["feePayer"], true);
        assert_eq!(json["splits"][0]["amount"], "100");
        assert!(json.get("permit2Address").is_none());
    }

    #[test]
    fn session_method_details_bps_splits() {
        let d = SessionMethodDetails {
            chain_id: 196,
            escrow_contract: "0xescrow".into(),
            channel_id: None,
            min_voucher_delta: Some("10000".into()),
            fee_payer: None,
            splits: Some(vec![SessionSplit {
                recipient: "0xsplit".into(),
                bps: 50,
                memo: None,
            }]),
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["escrowContract"], "0xescrow");
        assert_eq!(json["minVoucherDelta"], "10000");
        assert_eq!(json["splits"][0]["bps"], 50);
    }

    #[test]
    fn eip3009_authorization_camel_case() {
        let a = Eip3009Authorization {
            auth_type: Eip3009Authorization::TYPE.into(),
            from: "0xfrom".into(),
            to: "0xto".into(),
            value: "1000".into(),
            valid_after: "0".into(),
            valid_before: "9999999999".into(),
            nonce: "0xnonce".into(),
            signature: "0xsig".into(),
            splits: None,
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json["type"], "eip-3009");
        assert_eq!(json["validAfter"], "0");
        assert_eq!(json["validBefore"], "9999999999");
        assert!(json.get("splits").is_none());
    }

    #[test]
    fn voucher_domain_defaults() {
        let d = VoucherDomain::new(196, "0xescrow");
        assert_eq!(d.name, "EVM Payment Channel");
        assert_eq!(d.version, "1");
        assert_eq!(d.chain_id, 196);
    }
}
