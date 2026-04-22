//! Challenge builders for the `"evm"` method.
//!
//! `mpp::server::Mpp::charge()` in upstream is gated on the `tempo` feature and
//! hardcodes `method="tempo"`. Since we use `method="evm"`, we build the
//! `PaymentChallenge` directly using the method-agnostic
//! [`mpp::protocol::core::compute_challenge_id`] HMAC helper.

use mpp::protocol::core::{compute_challenge_id, Base64UrlJson, PaymentChallenge};
use mpp::protocol::intents::{ChargeRequest, SessionRequest};
use time::{Duration, OffsetDateTime};

use crate::types::{ChargeMethodDetails, SessionMethodDetails};

/// Method name for all challenges emitted by this SDK.
pub const METHOD_NAME: &str = "evm";

/// Intent name for charge challenges.
pub const INTENT_CHARGE: &str = "charge";

/// Intent name for session challenges.
pub const INTENT_SESSION: &str = "session";

/// Default challenge expiry window (5 minutes, matching mpp-rs).
pub const DEFAULT_EXPIRES_MINUTES: i64 = 5;

/// Build a `method="evm"` charge challenge.
///
/// The caller supplies the fully-populated [`ChargeRequest`] including
/// `methodDetails`. Use [`charge_request_with`] to construct one from typed
/// [`ChargeMethodDetails`].
pub fn build_charge_challenge(
    secret_key: &str,
    realm: &str,
    request: &ChargeRequest,
    expires: Option<&str>,
    description: Option<&str>,
) -> Result<PaymentChallenge, String> {
    let encoded =
        Base64UrlJson::from_typed(request).map_err(|e| format!("encode charge request: {}", e))?;

    let expires_owned;
    let expires = match expires {
        Some(e) => Some(e),
        None => {
            let t = OffsetDateTime::now_utc() + Duration::minutes(DEFAULT_EXPIRES_MINUTES);
            expires_owned = t
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| format!("format expires: {}", e))?;
            Some(expires_owned.as_str())
        }
    };

    let id = compute_challenge_id(
        secret_key,
        realm,
        METHOD_NAME,
        INTENT_CHARGE,
        encoded.raw(),
        expires,
        None,
        None,
    );

    Ok(PaymentChallenge {
        id,
        realm: realm.to_string(),
        method: METHOD_NAME.into(),
        intent: INTENT_CHARGE.into(),
        request: encoded,
        expires: expires.map(|s| s.to_string()),
        description: description.map(|s| s.to_string()),
        digest: None,
        opaque: None,
    })
}

/// Build a `method="evm"` session challenge.
pub fn build_session_challenge(
    secret_key: &str,
    realm: &str,
    request: &SessionRequest,
    expires: Option<&str>,
    description: Option<&str>,
) -> Result<PaymentChallenge, String> {
    let encoded =
        Base64UrlJson::from_typed(request).map_err(|e| format!("encode session request: {}", e))?;

    let expires_owned;
    let expires = match expires {
        Some(e) => Some(e),
        None => {
            let t = OffsetDateTime::now_utc() + Duration::minutes(DEFAULT_EXPIRES_MINUTES);
            expires_owned = t
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| format!("format expires: {}", e))?;
            Some(expires_owned.as_str())
        }
    };

    let id = compute_challenge_id(
        secret_key,
        realm,
        METHOD_NAME,
        INTENT_SESSION,
        encoded.raw(),
        expires,
        None,
        None,
    );

    Ok(PaymentChallenge {
        id,
        realm: realm.to_string(),
        method: METHOD_NAME.into(),
        intent: INTENT_SESSION.into(),
        request: encoded,
        expires: expires.map(|s| s.to_string()),
        description: description.map(|s| s.to_string()),
        digest: None,
        opaque: None,
    })
}

/// Assemble a `ChargeRequest` from base-unit amount + typed method details.
pub fn charge_request_with(
    amount_base_units: impl Into<String>,
    currency: impl Into<String>,
    recipient: impl Into<String>,
    details: ChargeMethodDetails,
) -> Result<ChargeRequest, String> {
    let details_json =
        serde_json::to_value(&details).map_err(|e| format!("encode method details: {}", e))?;
    Ok(ChargeRequest {
        amount: amount_base_units.into(),
        currency: currency.into(),
        decimals: None,
        recipient: Some(recipient.into()),
        description: None,
        external_id: None,
        method_details: Some(details_json),
    })
}

/// Assemble a `SessionRequest` from per-unit amount + typed method details.
pub fn session_request_with(
    amount_per_unit_base: impl Into<String>,
    currency: impl Into<String>,
    recipient: impl Into<String>,
    details: SessionMethodDetails,
) -> Result<SessionRequest, String> {
    let details_json =
        serde_json::to_value(&details).map_err(|e| format!("encode method details: {}", e))?;
    Ok(SessionRequest {
        amount: amount_per_unit_base.into(),
        currency: currency.into(),
        decimals: None,
        recipient: Some(recipient.into()),
        unit_type: None,
        suggested_deposit: None,
        method_details: Some(details_json),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_challenge_has_evm_method_and_charge_intent() {
        let req = charge_request_with(
            "1000000",
            "0xtoken",
            "0xpayee",
            ChargeMethodDetails {
                chain_id: 196,
                fee_payer: Some(true),
                permit2_address: None,
                memo: None,
                splits: None,
            },
        )
        .unwrap();
        let ch = build_charge_challenge("secret", "api.test", &req, None, None).unwrap();
        assert_eq!(ch.method.as_str(), "evm");
        assert_eq!(ch.intent.as_str(), "charge");
        assert!(!ch.id.is_empty());
        assert_eq!(ch.realm, "api.test");
    }

    #[test]
    fn session_challenge_has_evm_method_and_session_intent() {
        let req = session_request_with(
            "100",
            "0xtoken",
            "0xpayee",
            SessionMethodDetails {
                chain_id: 196,
                escrow_contract: "0xescrow".into(),
                channel_id: None,
                min_voucher_delta: Some("10000".into()),
                fee_payer: None,
                splits: None,
            },
        )
        .unwrap();
        let ch = build_session_challenge("secret", "api.test", &req, None, None).unwrap();
        assert_eq!(ch.method.as_str(), "evm");
        assert_eq!(ch.intent.as_str(), "session");
    }

    #[test]
    fn challenge_id_is_deterministic_for_same_inputs() {
        let req = charge_request_with(
            "1000",
            "0xtoken",
            "0xpayee",
            ChargeMethodDetails {
                chain_id: 196,
                fee_payer: None,
                permit2_address: None,
                memo: None,
                splits: None,
            },
        )
        .unwrap();
        let expiry = "2026-04-01T12:05:00Z";
        let a = build_charge_challenge("secret", "api.test", &req, Some(expiry), None).unwrap();
        let b = build_charge_challenge("secret", "api.test", &req, Some(expiry), None).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn different_secret_keys_yield_different_ids() {
        let req = charge_request_with(
            "1000",
            "0xtoken",
            "0xpayee",
            ChargeMethodDetails {
                chain_id: 196,
                fee_payer: None,
                permit2_address: None,
                memo: None,
                splits: None,
            },
        )
        .unwrap();
        let expiry = "2026-04-01T12:05:00Z";
        let a = build_charge_challenge("s1", "api", &req, Some(expiry), None).unwrap();
        let b = build_charge_challenge("s2", "api", &req, Some(expiry), None).unwrap();
        assert_ne!(a.id, b.id);
    }
}
