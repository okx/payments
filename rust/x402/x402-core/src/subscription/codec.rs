//! Codec: convert the buyer's `PAYMENT-SIGNATURE` payload into the flat
//! facilitator request bodies the Seller forwards.
//!
//! Pure structural mapping, no crypto. The Seller relays the buyer's
//! `payer`-signed structures verbatim, only renaming fields to the flat shape.

use base64::{engine::general_purpose::STANDARD as B64, Engine};

use crate::error::X402Error;
use crate::types::PaymentPayload;

use super::types::{
    AccessProof, ChangeSubscriptionRequest, CreateSubscriptionRequest, SubscriptionPayloadInner,
};

/// Extract the subscription inner payload from a decoded `PAYMENT-SIGNATURE`
/// `PaymentPayload`. Errors if the payload doesn't match the subscription shape.
pub fn unpack_subscription_payload(
    payload: &PaymentPayload,
) -> Result<SubscriptionPayloadInner, X402Error> {
    // Re-parse the generic JSON payload into the strongly-typed inner shape.
    let value = serde_json::to_value(&payload.payload)?;
    let inner: SubscriptionPayloadInner = serde_json::from_value(value)?;
    Ok(inner)
}

/// Decode an `APP-Access` header value (base64 JSON) into an [`AccessProof`].
/// Verification (ecrecover) is done in x402-evm.
pub fn decode_access_proof(header_value: &str) -> Result<AccessProof, X402Error> {
    let bytes = B64
        .decode(header_value.trim())
        .map_err(|e| X402Error::Other(format!("APP-Access base64 decode failed: {e}")))?;
    let proof: AccessProof = serde_json::from_slice(&bytes)?;
    Ok(proof)
}

/// Build the `POST /subscriptions` flat body from the buyer's inner payload.
/// Field renames: `permitSingle → permit`, `permitSingleSignature → permitSig`,
/// `termsSignature → termsSig`.
pub fn to_create_request(
    chain_index: u64,
    inner: SubscriptionPayloadInner,
    sync_settle: bool,
) -> CreateSubscriptionRequest {
    CreateSubscriptionRequest {
        chain_index,
        terms: inner.terms,
        permit: inner.permit_single,
        terms_sig: inner.terms_signature,
        permit_sig: inner.permit_single_signature,
        sync_settle,
    }
}

/// Build the `POST /subscriptions/change` flat body. `old_sub_id` is
/// informational; the facilitator uses `newTerms.changeFromSubId`.
pub fn to_change_request(
    chain_index: u64,
    old_sub_id: impl Into<String>,
    inner: SubscriptionPayloadInner,
    sync_settle: bool,
) -> ChangeSubscriptionRequest {
    ChangeSubscriptionRequest {
        chain_index,
        old_sub_id: old_sub_id.into(),
        new_terms: inner.terms,
        permit: inner.permit_single,
        terms_sig: inner.terms_signature,
        permit_sig: inner.permit_single_signature,
        sync_settle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PaymentRequirements;
    use serde_json::json;
    use std::collections::HashMap;

    fn buyer_payload() -> PaymentPayload {
        let payload: HashMap<String, serde_json::Value> = serde_json::from_value(json!({
            "permitSingle": {
                "details": { "token": "0xt", "amount": "60000000", "expiration": 1782000000u64, "nonce": 7 },
                "spender": "0xsub",
                "sigDeadline": "1750000000"
            },
            "permitSingleSignature": "0xpsig",
            "terms": {
                "payer": "0xp", "merchant": "0xm", "facilitator": "0xf", "token": "0xt",
                "amountPerPeriod": "5000000", "periodSec": 2592000, "maxPeriods": 12, "startAt": 0,
                "initialChargePeriods": 1, "initialChargeAmount": "5000000", "termsDeadline": 1750000000u64,
                "permitHash": "0xph", "salt": "0xsalt", "planTier": 2,
                "changeFromSubId": "0x00", "changeEffectiveAt": 0
            },
            "termsSignature": "0xtsig"
        }))
        .unwrap();

        PaymentPayload {
            x402_version: 2,
            resource: None,
            accepted: PaymentRequirements {
                scheme: "period".into(),
                network: "eip155:196".into(),
                asset: "0xt".into(),
                amount: "5000000".into(),
                pay_to: "0xm".into(),
                max_timeout_seconds: 600,
                extra: HashMap::new(),
            },
            payload,
            extensions: None,
        }
    }

    #[test]
    fn unpack_then_create_request_maps_and_renames() {
        let pp = buyer_payload();
        let inner = unpack_subscription_payload(&pp).unwrap();
        assert_eq!(inner.permit_single.details.nonce, 7);
        assert_eq!(inner.terms.plan_tier, 2);

        let req = to_create_request(196, inner, true);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["chainIndex"], 196);
        assert_eq!(v["permit"]["details"]["nonce"], 7);
        assert_eq!(v["permitSig"], "0xpsig");
        assert_eq!(v["termsSig"], "0xtsig");
        assert_eq!(v["terms"]["amountPerPeriod"], "5000000");
        assert_eq!(v["syncSettle"], true);
    }

    #[test]
    fn to_change_request_carries_old_sub_id() {
        let inner = unpack_subscription_payload(&buyer_payload()).unwrap();
        let req = to_change_request(196, "0xOld", inner, false);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["oldSubId"], "0xOld");
        assert_eq!(v["newTerms"]["planTier"], 2);
        assert_eq!(v["syncSettle"], false);
    }

    #[test]
    fn unpack_rejects_non_subscription_payload() {
        let mut pp = buyer_payload();
        pp.payload = HashMap::new(); // empty → missing required fields
        assert!(unpack_subscription_payload(&pp).is_err());
    }
}
