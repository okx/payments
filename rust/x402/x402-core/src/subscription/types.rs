//! Wire types for the `period` scheme (Seller side).
//!
//! Field names and order are canonical: the facilitator recomputes the EIP-712
//! digest from these, so renaming or reordering breaks signature verification.
//! Atomic amounts (`uint160`/`uint256`) are decimal strings; smaller uints use
//! their native width; `bytes32`/addresses are `0x`-hex strings.

use serde::{Deserialize, Serialize};

// ───────────────────────────── signed structures ───────────────────────────

/// `SubscriptionTerms` wire body: the 17 signed fields in EIP-712 order plus the
/// unsigned `planId`, which is carried on the wire but not part of the TypeHash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionTerms {
    pub payer: String,
    pub merchant: String,
    pub facilitator: String,
    pub token: String,
    pub amount_per_period: String,
    pub period_sec: u64,
    pub max_periods: u32,
    pub start_at: u64,
    pub initial_charge_periods: u32,
    pub initial_charge_amount: String,
    pub terms_deadline: u64,
    pub permit_hash: String,
    pub salt: String,
    /// bytes32 business plan id (`keccak256(plan.id)`); NOT signed, carried on the
    /// wire only. `default` keeps older payloads (without planId) decodable.
    #[serde(default)]
    pub plan_id: String,
    pub plan_tier: u8,
    pub change_from_sub_id: String,
    pub change_effective_at: u8,
    /// Period mode: 0 = fixed_seconds (`periodSec > 0`), 1 = calendar_month
    /// (`periodSec` must be 0). `default` decodes older payloads as fixed_seconds.
    #[serde(default)]
    pub period_mode: u8,
}

/// Permit2 `PermitDetails` (AllowanceTransfer).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermitDetails {
    pub token: String,
    pub amount: String,
    pub expiration: u64,
    pub nonce: u64,
}

/// Permit2 `PermitSingle` — `spender` is the subscription contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermitSingle {
    pub details: PermitDetails,
    pub spender: String,
    pub sig_deadline: String,
}

/// `CancelAuth` (cancel an active subscription). `action` 0 = cancel_subscription,
/// `initiator` 0 = payer / 1 = merchant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelAuth {
    pub action: u8,
    pub sub_id: String,
    pub initiator: u8,
    pub nonce: String,
    pub deadline: u64,
    pub signature: String,
}

/// `PendingChangeCancelAuth` (cancel a not-yet-effective downgrade, payer only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeCancelAuth {
    pub sub_id: String,
    /// Target of the pending downgrade being canceled; must equal the current
    /// PENDING `newSubId`. Part of the signed digest, so a mismatch fails
    /// verification.
    pub new_sub_id: String,
    pub nonce: String,
    pub deadline: u64,
    pub signature: String,
}

/// `APP-Access` AccessProof — base64(JSON) sent by the buyer on every access to
/// a protected route. The Seller verifies it (x402-evm `verify_access_proof`):
/// kind + ±300s window + `ecrecover == payer` + active subscription state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccessProof {
    /// Always `"subscription-id"`.
    pub kind: String,
    pub sub_id: String,
    pub payer: String,
    /// Unix seconds.
    pub timestamp: u64,
    /// 0x-prefixed 65-byte secp256k1 EIP-191 personal_sign, signer == payer.
    pub signature: String,
}

// ───────────────── PAYMENT-SIGNATURE payload (Buyer → Seller) ───────────────

/// The inner `payload` object of the buyer's `PAYMENT-SIGNATURE` header for a
/// subscribe/change. The Seller [`super::codec`]-converts this into a flat
/// facilitator request body.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPayloadInner {
    pub permit_single: PermitSingle,
    /// 0x-prefixed 65-byte secp256k1, signer == payer (Permit2 domain).
    pub permit_single_signature: String,
    pub terms: SubscriptionTerms,
    /// 0x-prefixed 65-byte secp256k1, signer == payer (subscription domain).
    pub terms_signature: String,
}

// ─────────────── Facilitator flat request bodies (Seller → SA) ──────────────

/// `POST /subscriptions` body. Field renames vs the buyer payload:
/// `permitSingle → permit`, `permitSingleSignature → permitSig`,
/// `termsSignature → termsSig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionRequest {
    pub chain_index: u64,
    pub terms: SubscriptionTerms,
    pub permit: PermitSingle,
    pub terms_sig: String,
    pub permit_sig: String,
    pub sync_settle: bool,
}

/// `POST /subscriptions/change` body. `oldSubId` is informational; the
/// facilitator uses `newTerms.changeFromSubId`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSubscriptionRequest {
    pub chain_index: u64,
    pub old_sub_id: String,
    pub new_terms: SubscriptionTerms,
    pub permit: PermitSingle,
    pub terms_sig: String,
    pub permit_sig: String,
    pub sync_settle: bool,
}

/// `POST /subscriptions/charge` body. `subId` rides in the body (AK auth forbids
/// path placeholders).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionChargeReq {
    pub sub_id: String,
    pub sync_settle: bool,
}

/// `POST /subscriptions/cancel` body. `subId` in the body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelSubscriptionRequest {
    pub sub_id: String,
    pub cancel_auth: CancelAuth,
    pub sync_settle: bool,
}

/// `POST /subscriptions/cancel-pending-change` body. `subId` in the body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelPendingChangeRequest {
    pub sub_id: String,
    pub cancel_auth: PendingChangeCancelAuth,
    pub sync_settle: bool,
}

/// `POST /subscriptions/finalize-expired` body. `subId` in the body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionFinalizeExpiredReq {
    pub sub_id: String,
}

// ──────────────────────── Facilitator response bodies ───────────────────────

/// `POST /subscriptions` response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionResponse {
    pub sub_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub state: u8,
}

/// `POST /subscriptions/{subId}/charge` response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeResponse {
    pub sub_id: String,
    pub period: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub state: u8,
    #[serde(default)]
    pub plan_change_triggered: bool,
    /// Set on the downgrade-effective period: the successor subId.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_sub_id: Option<String>,
}

/// `POST /subscriptions/change` response data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResponse {
    pub new_sub_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub state: u8,
}

/// `POST .../cancel`, `.../cancel-pending-change` and `.../finalize-expired`
/// response data. `state` and `txHash` are optional (returned as `null` when
/// there is no on-chain transition).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxResultResponse {
    pub sub_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(default)]
    pub state: Option<u8>,
}

/// `GET /subscriptions/{subId}` status. Extra/derived fields are optional so
/// partial backend responses still parse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionStatus {
    pub sub_id: String,
    pub state: u8,
    #[serde(default)]
    pub payer: String,
    #[serde(default)]
    pub merchant: String,
    #[serde(default)]
    pub token: String,
    /// Per-period amount, atomic (uint160).
    #[serde(default)]
    pub amount_per_period: String,
    /// Fixed-seconds period length; 0 in calendar-month mode.
    #[serde(default)]
    pub period_sec: u64,
    #[serde(default)]
    pub max_periods: u32,
    #[serde(default)]
    pub start_at: u64,
    /// Period mode: 0 = fixed_seconds, 1 = calendar_month.
    #[serde(default)]
    pub period_mode: u8,
    /// Calendar-month billing anchor (Unix secs); 0 = pending backfill;
    /// ignored in fixed mode.
    #[serde(default)]
    pub billing_anchor_at: u64,
    #[serde(default)]
    pub last_charged_period: u32,
    /// Cumulative amount pulled across all charges, atomic.
    #[serde(default)]
    pub total_pulled: String,
    /// Business plan id (plain string, e.g. `"pro_monthly"`); empty if the
    /// backend omits it. Lets the seller recover the plan without a local record.
    #[serde(default)]
    pub plan_id: String,
    /// Plan tier (`> 0`); `0` if unknown.
    #[serde(default)]
    pub plan_tier: u8,
    #[serde(default)]
    pub current_period: u32,
    /// Real elapsed period number, un-clamped: `elapsedPeriods > maxPeriods`
    /// means the service window has ended. Use this, not the clamped
    /// `current_period`, to judge expiry.
    #[serde(default)]
    pub elapsed_periods: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_chargeable_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_to_sub_id: Option<String>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub service_ended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_plan_change: Option<PendingPlanChange>,
}

/// Pending (not-yet-effective) downgrade descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPlanChange {
    pub sub_id: String,
    pub new_sub_id: String,
    pub effective_from_period: u32,
    pub state: u8,
}

/// One charge ledger entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCharge {
    pub sub_id: String,
    pub period: u32,
    /// `SubscriptionChargeType`: 1 initial / 2 periodic / 3 downgrade_first_period /
    /// 4 finalize_expired_marker.
    pub charge_type: u8,
    pub amount: String,
    /// `SubscriptionChargeState`: 0 pending / 1 success / 2 failed.
    pub state: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(default)]
    pub plan_change_triggered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_sub_id: Option<String>,
}

/// `GET /subscriptions/{subId}/charges` response data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChargesResponse {
    #[serde(default)]
    pub charges: Vec<SubscriptionCharge>,
}

// ────────────────────────────── enums (as u8) ──────────────────────────────

/// `SubscriptionState`: contract NONE/ACTIVE/COMPLETED/CANCELED/CHANGED plus
/// SA-local PENDING(0, occupies NONE slot)/FAILED(99).
pub mod subscription_state {
    pub const PENDING: u8 = 0;
    pub const ACTIVE: u8 = 1;
    pub const COMPLETED: u8 = 2;
    pub const CANCELED: u8 = 3;
    pub const CHANGED: u8 = 4;
    pub const FAILED: u8 = 99;
}

/// `ChangeEffectiveAt`.
pub mod change_effective_at {
    pub const NONE: u8 = 0;
    pub const IMMEDIATE: u8 = 1; // upgrade
    pub const PERIOD_END: u8 = 2; // downgrade
}

/// `CancelInitiator`.
pub mod cancel_initiator {
    pub const PAYER: u8 = 0;
    pub const MERCHANT: u8 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn terms_round_trip_camel_case_18_fields() {
        let terms = SubscriptionTerms {
            payer: "0xp".into(),
            merchant: "0xm".into(),
            facilitator: "0xf".into(),
            token: "0xt".into(),
            amount_per_period: "5000000".into(),
            period_sec: 0,
            max_periods: 12,
            start_at: 0,
            initial_charge_periods: 1,
            initial_charge_amount: "5000000".into(),
            terms_deadline: 1_750_000_000,
            permit_hash: "0xph".into(),
            salt: "0xsalt".into(),
            plan_id: "0xplan".into(),
            plan_tier: 2,
            change_from_sub_id: "0x00".into(),
            change_effective_at: 0,
            period_mode: 1,
        };
        let v = serde_json::to_value(&terms).unwrap();
        assert_eq!(v["amountPerPeriod"], "5000000");
        assert_eq!(v["changeFromSubId"], "0x00");
        assert_eq!(v["changeEffectiveAt"], 0);
        assert_eq!(v["periodMode"], 1);
        // planId is on the wire (unsigned) but never in the EIP-712 digest.
        assert_eq!(v["planId"], "0xplan");
        assert_eq!(v.as_object().unwrap().len(), 18, "17 signed + planId");
        let back: SubscriptionTerms = serde_json::from_value(v).unwrap();
        assert_eq!(back, terms);
    }

    #[test]
    fn payload_inner_uses_permit_single_keys() {
        let v = json!({
            "permitSingle": { "details": {"token":"0xt","amount":"60000000","expiration":1782000000u64,"nonce":7}, "spender":"0xsub", "sigDeadline":"1750000000" },
            "permitSingleSignature": "0xpsig",
            "terms": { "payer":"0xp","merchant":"0xm","facilitator":"0xf","token":"0xt","amountPerPeriod":"5000000","periodSec":2592000,"maxPeriods":12,"startAt":0,"initialChargePeriods":1,"initialChargeAmount":"5000000","termsDeadline":1750000000u64,"permitHash":"0xph","salt":"0xsalt","planTier":2,"changeFromSubId":"0x00","changeEffectiveAt":0,"periodMode":0 },
            "termsSignature": "0xtsig"
        });
        let inner: SubscriptionPayloadInner = serde_json::from_value(v).unwrap();
        assert_eq!(inner.permit_single.details.nonce, 7);
        assert_eq!(inner.permit_single_signature, "0xpsig");
        assert_eq!(inner.terms_signature, "0xtsig");
        assert_eq!(inner.terms.plan_tier, 2);
    }

    #[test]
    fn create_request_renames_to_flat_body() {
        let req = CreateSubscriptionRequest {
            chain_index: 196,
            terms: SubscriptionTerms {
                payer: "0xp".into(), merchant: "0xm".into(), facilitator: "0xf".into(),
                token: "0xt".into(), amount_per_period: "1".into(), period_sec: 1, max_periods: 1,
                start_at: 0, initial_charge_periods: 0, initial_charge_amount: "0".into(),
                terms_deadline: 1, permit_hash: "0x".into(), salt: "0x".into(),
                plan_id: "0xplan".into(), plan_tier: 1,
                change_from_sub_id: "0x".into(), change_effective_at: 0, period_mode: 0,
            },
            permit: PermitSingle {
                details: PermitDetails { token: "0xt".into(), amount: "1".into(), expiration: 1, nonce: 0 },
                spender: "0xsub".into(), sig_deadline: "1".into(),
            },
            terms_sig: "0xts".into(),
            permit_sig: "0xps".into(),
            sync_settle: true,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["chainIndex"], 196);
        assert_eq!(v["termsSig"], "0xts");
        assert_eq!(v["permitSig"], "0xps");
        assert_eq!(v["syncSettle"], true);
        assert!(v.get("permit").is_some());
    }

    #[test]
    fn charge_response_parses_plan_change() {
        let r: ChargeResponse = serde_json::from_value(json!({
            "subId": "0xold", "period": 4, "txHash": "0xabc", "state": 1,
            "planChangeTriggered": true, "newSubId": "0xnew"
        }))
        .unwrap();
        assert_eq!(r.period, 4);
        assert!(r.plan_change_triggered);
        assert_eq!(r.new_sub_id.as_deref(), Some("0xnew"));
    }
}
