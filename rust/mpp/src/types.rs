//! Data types for MPP EVM SDK.
//!
//! Covers: challenge method details, credential payloads, receipts, channel state,
//! and EIP-712 voucher domain constants.

use serde::{Deserialize, Serialize};

// ==================== SA API Response Wrapper ====================

/// SA API unified response: `{ code, data, msg }`.
///
/// `code` is `i64` rather than `u32` because the SA backend returns
/// `code: -1` ("unknown error") on some error paths; parsing as `u32`
/// would fail and swallow the original error message. Business codes
/// themselves are non-negative integers (8000, 70000-70014, ...) so
/// `as u32` casts cleanly downstream.
#[derive(Debug, Clone, Deserialize)]
pub struct SaApiResponse<T> {
    pub code: i64,
    pub data: Option<T>,
    #[serde(default)]
    pub msg: String,
}

// ==================== Constants ====================

/// Default X Layer chain ID.
pub const DEFAULT_CHAIN_ID: u64 = 196;

// EIP-712 typed structs (Voucher / SettleAuthorization / CloseAuthorization)
// and domain constants have moved to the `crate::eip712` module. Use:
//   - `crate::eip712::voucher::Voucher` (verification struct)
//   - `crate::eip712::authorization::{SettleAuthorization, CloseAuthorization}` (signing structs)
//   - `crate::eip712::domain::{VOUCHER_DOMAIN_NAME, VOUCHER_DOMAIN_VERSION, build_domain}`

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

/// Receipt returned by POST `/session/{open,topUp,settle,close}`.
///
/// Required fields: `method / intent / status / timestamp / channelId /
/// chainId / reference / deposit`. The new (post-cleanup) protocol no
/// longer returns `challengeId / acceptedCumulative / spent /
/// confirmations / units`; they're kept as `Option` for back-compat
/// (deserialize as `None`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReceipt {
    pub method: String,
    pub intent: String,
    pub status: String,
    pub timestamp: String,
    pub chain_id: u64,
    pub channel_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Current on-chain known deposit for this channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deposit: Option<String>,

    /// Optional fields (the new protocol skips them; kept as `Option`
    /// for deserialization compatibility).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_cumulative: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<u64>,
}

// ==================== Channel Status (spec §8.4) ====================

/// Response from GET `/session/status`.
///
/// `cumulativeAmount` was removed from the spec ("only settle updates
/// it"). The field is kept as `Option` for backwards compatibility with
/// old responses; new SA API responses always have it as `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatus {
    pub channel_id: String,
    pub payer: String,
    pub payee: String,
    pub token: String,
    pub deposit: String,
    pub settled_on_chain: String,
    pub session_status: String,
    pub remaining_balance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cumulative_amount: Option<String>,
}

// ==================== Settle / Close request payloads ====================
//
// Body shape for merchant-initiated settle / close requests (no challenge wrapper):
// settle:
//   { "action": "settle", "channelId", "cumulativeAmount", "voucherSignature",
//     "payeeSignature", "nonce", "deadline" }
// close: same shape; the waiver branch sends `voucherSignature: ""` (the
// server triggers waiver on either `cumulativeAmount <= settledOnChain`
// or `voucherSignature == ""`).

/// `POST /session/settle` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleRequestPayload {
    /// Conventionally `"settle"`. The server does not enforce this strictly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    /// Channel ID (bytes32 hex, 0x-prefixed).
    pub channel_id: String,

    /// Cumulative settle amount (uint128 decimal string).
    pub cumulative_amount: String,

    /// EIP-712 Voucher signature (signed by payer / authorizedSigner).
    /// 65-byte `r ‖ s ‖ v` hex.
    pub voucher_signature: String,

    /// EIP-712 SettleAuthorization signature (signed by payee).
    /// 65-byte `r ‖ s ‖ v` hex.
    pub payee_signature: String,

    /// uint256 decimal string. `(payee, channelId, nonce)` is the
    /// contract-level used-set key.
    pub nonce: String,

    /// Signature expiry — uint256 decimal string (unix seconds; SDK
    /// defaults to `U256::MAX`).
    pub deadline: String,
}

/// `POST /session/close` request body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    pub channel_id: String,
    pub cumulative_amount: String,

    /// EIP-712 Voucher signature. Normal branch: 65-byte `r ‖ s ‖ v` hex.
    /// Waiver branch (`cumulativeAmount <= settledOnChain` or no local
    /// voucher): empty string `""`.
    pub voucher_signature: String,

    /// EIP-712 CloseAuthorization signature (signed by payee).
    pub payee_signature: String,

    pub nonce: String,
    pub deadline: String,
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
        // Per the [Pay] MPP EVM API plan: open response has no "spent" field.
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
    fn settle_request_payload_serializes_camel_case() {
        let p = SettleRequestPayload {
            action: Some("settle".into()),
            channel_id: "0xabc".into(),
            cumulative_amount: "250000".into(),
            voucher_signature: "0xvoucher".into(),
            payee_signature: "0xpayee".into(),
            nonce: "17890324512398".into(),
            deadline:
                "115792089237316195423570985008687907853269984665640564039457584007913129639935"
                    .into(),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["action"], "settle");
        assert_eq!(json["channelId"], "0xabc");
        assert_eq!(json["voucherSignature"], "0xvoucher");
        assert_eq!(json["payeeSignature"], "0xpayee");
        assert_eq!(json["nonce"], "17890324512398");
        // deadline serializes as String, matching the protocol (uint256 decimal string).
        assert!(json["deadline"].is_string());
    }

    #[test]
    fn close_request_payload_omits_action_when_none() {
        let p = CloseRequestPayload {
            action: None,
            channel_id: "0xabc".into(),
            cumulative_amount: "500000".into(),
            voucher_signature: "0xvsig".into(),
            payee_signature: "0xpsig".into(),
            nonce: "1".into(),
            deadline: "999".into(),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(
            json.get("action").is_none(),
            "action must be omitted when None"
        );
        assert_eq!(json["channelId"], "0xabc");
    }

    #[test]
    fn session_receipt_accepts_minimal_shape() {
        // Minimal response (no challengeId / acceptedCumulative / spent).
        let json = r#"{
            "method":"evm",
            "intent":"session",
            "status":"success",
            "timestamp":"2026-04-01T12:00:00Z",
            "chainId":196,
            "channelId":"0xdead",
            "deposit":"1000"
        }"#;
        let r: SessionReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(r.channel_id, "0xdead");
        assert_eq!(r.deposit.as_deref(), Some("1000"));
        assert!(r.challenge_id.is_none());
        assert!(r.accepted_cumulative.is_none());
        assert!(r.spent.is_none());
    }

    #[test]
    fn channel_status_without_cumulative_amount_draft2() {
        let json = r#"{
            "channelId":"0xabc",
            "payer":"0xp", "payee":"0xq", "token":"0xt",
            "deposit":"10000", "settledOnChain":"500",
            "sessionStatus":"OPEN", "remainingBalance":"9500"
        }"#;
        let s: ChannelStatus = serde_json::from_str(json).unwrap();
        assert_eq!(s.session_status, "OPEN");
        assert!(
            s.cumulative_amount.is_none(),
            "cumulativeAmount must not be returned"
        );
    }
}
