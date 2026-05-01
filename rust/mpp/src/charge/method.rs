//! EvmChargeMethod: ChargeMethod implementation backed by SA API.
//!
//! Replaces mpp-rs TempoChargeMethod. Forwards credentials to SA API for
//! verification and on-chain settlement.
//!
//! Two modes, routed by `payload.type`:
//! - `"transaction"` → SA API broadcasts `transferWithAuthorization` on-chain
//! - `"hash"` → SA API verifies a client-broadcasted transaction
//!
//! Splits (multi-recipient charges) are passed through inside
//! `payload.authorization.splits[]` — this layer does not re-parse or validate
//! them; SA API owns split verification.

use std::future::Future;
use std::sync::Arc;

use mpp::protocol::core::{PaymentCredential, Receipt};
use mpp::protocol::intents::ChargeRequest;
use mpp::protocol::traits::{ChargeMethod, ErrorCode, VerificationError};

use crate::sa_client::SaApiClient;
use crate::types::ChargeMethodDetails;

const PAYLOAD_TYPE_TRANSACTION: &str = "transaction";
const PAYLOAD_TYPE_HASH: &str = "hash";

/// Grace window (seconds) added on top of `challenge.expires` when
/// validating an EIP-3009 `validBefore`. Allows for SA broadcast latency
/// after challenge expiry without weakening the binding meaningfully.
const VALID_BEFORE_GRACE_SECS: u64 = 60;

/// EVM Charge Method backed by OKX SA API.
///
/// ```rust,no_run
/// use mpp_evm::{EvmChargeMethod, OkxSaApiClient};
/// use std::sync::Arc;
///
/// let sa_client = Arc::new(OkxSaApiClient::new(
///     "api-key".into(), "secret".into(), "passphrase".into(),
/// ));
/// let charge_method = EvmChargeMethod::new(sa_client);
/// ```
#[derive(Clone)]
pub struct EvmChargeMethod {
    sa_client: Arc<dyn SaApiClient>,
}

impl EvmChargeMethod {
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self { sa_client }
    }
}

impl ChargeMethod for EvmChargeMethod {
    fn method(&self) -> &str {
        "evm"
    }

    fn verify(
        &self,
        credential: &PaymentCredential,
        request: &ChargeRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let sa_client = self.sa_client.clone();
        let credential = credential.clone();
        let request = request.clone();
        let challenge_id = credential.challenge.id.clone();

        async move {
            let payload_type = credential
                .payload
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // C1: bind the credential to the challenge BEFORE forwarding
            // to SA. Without this, an attacker who holds any valid
            // EIP-3009 sig (e.g. for an unrelated transfer to their own
            // wallet) could pair it with a legitimate cheap-resource
            // challenge and unlock any resource — SA only checks that the
            // sig recovers, not that the authorization fields match the
            // challenge.
            //
            // Hash mode: no on-chain authorization in the credential —
            // SA verifies the broadcast tx directly. Skip binding here.
            if payload_type == PAYLOAD_TYPE_TRANSACTION {
                bind_authorization_to_request(
                    &credential.payload,
                    &request,
                    credential.challenge.expires.as_deref(),
                )?;
            }

            // Both `charge/settle` and `charge/verifyHash` need the
            // challenge object, so we forward the full credential
            // (challenge + payload + source) to SA API as-is.
            let credential_json = serde_json::to_value(&credential)
                .map_err(|e| VerificationError::new(format!("serialize credential: {}", e)))?;

            let receipt = match payload_type {
                PAYLOAD_TYPE_TRANSACTION => sa_client.charge_settle(&credential_json).await,
                PAYLOAD_TYPE_HASH => sa_client.charge_verify_hash(&credential_json).await,
                other => {
                    return Err(VerificationError::with_code(
                        format!("unsupported charge payload type: {:?}", other),
                        ErrorCode::InvalidPayload,
                    ));
                }
            };

            receipt
                .map(|r| Receipt::success("evm", &r.reference))
                .map_err(|sa_err| {
                    let problem = sa_err.to_problem_details(Some(&challenge_id));
                    VerificationError::new(problem.detail)
                })
        }
    }
}

/// Verify that `payload.authorization` (transaction mode) matches the
/// fields the server signed into the challenge `request`. Closes the
/// "any valid EIP-3009 sig + any cheap challenge → unlock anything"
/// attack (C1) and, when `challenge_expires` is present, also enforces
/// `validBefore >= challenge.expires + grace` (H7).
///
/// Strict-equality checks:
/// - `authorization.to == request.recipient` (case-insensitive Address).
/// - `authorization.value + sum(splits[*].value) == request.amount`.
/// - Each `authorization.splits[i].{to, value}` matches
///   `request.method_details.splits[i].{recipient, amount}` 1:1.
/// - `authorization.validBefore >= challenge.expires + 60s grace`.
///
/// Currency / chain_id are bound implicitly by the EIP-712 domain of the
/// signature itself: a sig targeting the wrong token / chain will fail
/// at the on-chain `transferWithAuthorization` call. We therefore don't
/// re-check them here.
fn bind_authorization_to_request(
    payload: &serde_json::Value,
    request: &ChargeRequest,
    challenge_expires: Option<&str>,
) -> Result<(), VerificationError> {
    let invalid = |msg: String| VerificationError::with_code(msg, ErrorCode::InvalidPayload);

    let auth = payload
        .get("authorization")
        .ok_or_else(|| invalid("transaction-mode payload missing `authorization`".into()))?;

    // recipient binding
    let recipient = request
        .recipient
        .as_deref()
        .ok_or_else(|| invalid("challenge.request.recipient missing".into()))?;
    let auth_to = auth
        .get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("authorization.to missing".into()))?;
    if !auth_to.eq_ignore_ascii_case(recipient) {
        return Err(invalid(format!(
            "authorization.to {auth_to} != challenge.recipient {recipient}"
        )));
    }

    // amount binding (primary value + sum of split values must equal request.amount)
    let request_amount: u128 = request
        .amount
        .parse()
        .map_err(|e| invalid(format!("challenge.request.amount not a u128: {e}")))?;
    let auth_value_str = auth
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| invalid("authorization.value missing".into()))?;
    let auth_value: u128 = auth_value_str
        .parse()
        .map_err(|e| invalid(format!("authorization.value not a u128: {e}")))?;

    let mut total = auth_value;
    let auth_splits = auth.get("splits").and_then(|v| v.as_array());
    let request_splits: Vec<crate::types::ChargeSplit> = request
        .method_details
        .as_ref()
        .map(|md| serde_json::from_value::<ChargeMethodDetails>(md.clone()))
        .transpose()
        .map_err(|e| invalid(format!("challenge.method_details deserialise: {e}")))?
        .and_then(|md| md.splits)
        .unwrap_or_default();

    match (auth_splits, request_splits.as_slice()) {
        (None, []) => {} // no splits anywhere — fine
        (Some(_), []) => {
            return Err(invalid(
                "authorization.splits present but challenge has no splits".into(),
            ));
        }
        (None, non_empty) if !non_empty.is_empty() => {
            return Err(invalid(
                "challenge declared splits but authorization.splits missing".into(),
            ));
        }
        (Some(auth_splits), req_splits) => {
            if auth_splits.len() != req_splits.len() {
                return Err(invalid(format!(
                    "splits length mismatch: authorization has {}, challenge has {}",
                    auth_splits.len(),
                    req_splits.len()
                )));
            }
            for (i, (sp_auth, sp_req)) in auth_splits.iter().zip(req_splits.iter()).enumerate() {
                let to = sp_auth
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| invalid(format!("authorization.splits[{i}].to missing")))?;
                if !to.eq_ignore_ascii_case(&sp_req.recipient) {
                    return Err(invalid(format!(
                        "authorization.splits[{i}].to {to} != challenge.splits[{i}].recipient {}",
                        sp_req.recipient
                    )));
                }
                let value_str = sp_auth
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| invalid(format!("authorization.splits[{i}].value missing")))?;
                if value_str != sp_req.amount {
                    return Err(invalid(format!(
                        "authorization.splits[{i}].value {value_str} != challenge.splits[{i}].amount {}",
                        sp_req.amount
                    )));
                }
                let v: u128 = value_str.parse().map_err(|e| {
                    invalid(format!("authorization.splits[{i}].value not u128: {e}"))
                })?;
                total = total
                    .checked_add(v)
                    .ok_or_else(|| invalid("split sum overflow".into()))?;
            }
        }
        _ => unreachable!(),
    }

    if total != request_amount {
        return Err(invalid(format!(
            "authorization total {total} (primary + splits) != challenge.amount {request_amount}"
        )));
    }

    // H7: validBefore must extend beyond challenge.expires + grace.
    if let Some(expires_str) = challenge_expires {
        use time::format_description::well_known::Rfc3339;
        use time::OffsetDateTime;
        let expires = OffsetDateTime::parse(expires_str, &Rfc3339).map_err(|e| {
            invalid(format!(
                "challenge.expires not RFC3339: {expires_str:?} ({e})"
            ))
        })?;
        let expires_unix = expires.unix_timestamp();
        if expires_unix < 0 {
            return Err(invalid(
                "challenge.expires before Unix epoch (negative timestamp)".into(),
            ));
        }
        let valid_before_str = auth
            .get("validBefore")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid("authorization.validBefore missing".into()))?;
        let valid_before: u64 = valid_before_str
            .parse()
            .map_err(|e| invalid(format!("authorization.validBefore not u64: {e}")))?;
        let min_required = (expires_unix as u64).saturating_add(VALID_BEFORE_GRACE_SECS);
        if valid_before < min_required {
            return Err(invalid(format!(
                "authorization.validBefore {valid_before} < challenge.expires + grace ({min_required})"
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SaApiError;
    use crate::types::{ChannelStatus, ChargeReceipt, SessionReceipt};
    use async_trait::async_trait;
    use mpp::protocol::core::{Base64UrlJson, ChallengeEcho};
    use std::sync::Mutex;

    /// Mock SA client that records the last credential sent to each endpoint.
    #[derive(Default)]
    struct MockSa {
        last_settle: Mutex<Option<serde_json::Value>>,
        last_verify_hash: Mutex<Option<serde_json::Value>>,
        next_error: Mutex<Option<SaApiError>>,
    }

    impl MockSa {
        fn with_error(err: SaApiError) -> Self {
            Self {
                next_error: Mutex::new(Some(err)),
                ..Default::default()
            }
        }

        fn take_settle(&self) -> serde_json::Value {
            self.last_settle
                .lock()
                .unwrap()
                .take()
                .expect("no settle recorded")
        }

        fn take_verify_hash(&self) -> serde_json::Value {
            self.last_verify_hash
                .lock()
                .unwrap()
                .take()
                .expect("no verifyHash recorded")
        }
    }

    fn mock_charge_receipt() -> ChargeReceipt {
        ChargeReceipt {
            method: "evm".into(),
            reference: "0xtxhash".into(),
            status: "success".into(),
            timestamp: "2026-04-01T12:00:00Z".into(),
            chain_id: 196,
            confirmations: None,
            challenge_id: Some("ch-1".into()),
            external_id: None,
        }
    }

    #[async_trait]
    impl SaApiClient for MockSa {
        async fn charge_settle(
            &self,
            cred: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            *self.last_settle.lock().unwrap() = Some(cred.clone());
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            Ok(mock_charge_receipt())
        }
        async fn charge_verify_hash(
            &self,
            cred: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            *self.last_verify_hash.lock().unwrap() = Some(cred.clone());
            if let Some(e) = self.next_error.lock().unwrap().take() {
                return Err(e);
            }
            Ok(mock_charge_receipt())
        }
        async fn session_open(&self, _c: &serde_json::Value) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _c: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(
            &self,
            _payload: &crate::types::SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_close(
            &self,
            _payload: &crate::types::CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_status(&self, _cid: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!()
        }
    }

    fn credential_with_payload(payload: serde_json::Value) -> PaymentCredential {
        PaymentCredential {
            challenge: ChallengeEcho {
                id: "ch-1".into(),
                realm: "test".into(),
                method: "evm".into(),
                intent: "charge".into(),
                request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
                expires: None,
                digest: None,
                opaque: None,
            },
            source: None,
            payload,
        }
    }

    fn dummy_request() -> ChargeRequest {
        ChargeRequest {
            amount: "1000".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            description: None,
            external_id: None,
            method_details: None,
        }
    }

    #[tokio::test]
    async fn transaction_payload_routes_to_charge_settle() {
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock.clone());
        // Authorization fields must match dummy_request() (recipient=0xPayee, amount=1000).
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "1000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0xn", "signature": "0xsig"
            }
        });
        let credential = credential_with_payload(payload);
        let receipt = method.verify(&credential, &dummy_request()).await.unwrap();
        assert_eq!(receipt.reference, "0xtxhash");
        let sent = mock.take_settle();
        assert_eq!(sent["payload"]["type"], "transaction");
    }

    #[tokio::test]
    async fn hash_payload_routes_to_verify_hash() {
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock.clone());
        let payload = serde_json::json!({ "type": "hash", "hash": "0xchainhash" });
        let mut credential = credential_with_payload(payload);
        credential.source = Some("did:pkh:eip155:196:0xfrom".into());
        let receipt = method.verify(&credential, &dummy_request()).await.unwrap();
        assert_eq!(receipt.reference, "0xtxhash");
        let sent = mock.take_verify_hash();
        assert_eq!(sent["payload"]["type"], "hash");
        assert_eq!(sent["payload"]["hash"], "0xchainhash");
    }

    #[tokio::test]
    async fn splits_are_forwarded_inside_authorization() {
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock.clone());
        // Per spec §8.2, splits sit under payload.authorization.splits[] with
        // each entry carrying its own signature. Total (primary + splits)
        // must equal challenge.amount (post-C1 binding).
        // Use ChargeMethodDetails so request.method_details declares splits
        // matching payload.authorization.splits[*].
        let request = ChargeRequest {
            amount: "1000000".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            description: None,
            external_id: None,
            method_details: Some(
                serde_json::to_value(crate::types::ChargeMethodDetails {
                    chain_id: 196,
                    fee_payer: None,
                    permit2_address: None,
                    memo: None,
                    splits: Some(vec![
                        crate::types::ChargeSplit {
                            amount: "50000".into(),
                            recipient: "0xsplit1".into(),
                            memo: None,
                        },
                        crate::types::ChargeSplit {
                            amount: "10000".into(),
                            recipient: "0xsplit2".into(),
                            memo: None,
                        },
                    ]),
                })
                .unwrap(),
            ),
        };
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "940000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0x1111", "signature": "0xprimarysig",
                "splits": [
                    {
                        "from": "0xfrom", "to": "0xsplit1", "value": "50000",
                        "validAfter": "0", "validBefore": "9999999999",
                        "nonce": "0x2222", "signature": "0xsplit1sig"
                    },
                    {
                        "from": "0xfrom", "to": "0xsplit2", "value": "10000",
                        "validAfter": "0", "validBefore": "9999999999",
                        "nonce": "0x3333", "signature": "0xsplit2sig"
                    }
                ]
            }
        });
        let credential = credential_with_payload(payload);
        method.verify(&credential, &request).await.unwrap();

        let sent = mock.take_settle();
        let splits = &sent["payload"]["authorization"]["splits"];
        assert_eq!(splits.as_array().unwrap().len(), 2);
        assert_eq!(splits[0]["to"], "0xsplit1");
        assert_eq!(splits[0]["value"], "50000");
        assert_eq!(splits[0]["signature"], "0xsplit1sig");
        assert_eq!(splits[1]["to"], "0xsplit2");
        // Nonces must remain independent per split.
        assert_ne!(splits[0]["nonce"], splits[1]["nonce"]);
    }

    #[tokio::test]
    async fn unknown_payload_type_returns_invalid_payload() {
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let payload = serde_json::json!({ "type": "weird" });
        let credential = credential_with_payload(payload);
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unsupported charge payload type"));
    }

    #[tokio::test]
    async fn sa_error_propagates_as_verification_error() {
        let mock = Arc::new(MockSa::with_error(SaApiError::new(70004, "bad sig")));
        let method = EvmChargeMethod::new(mock);
        // Authorization must satisfy C1 binding so we actually reach SA.
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "1000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0xn", "signature": "0xsig"
            }
        });
        let credential = credential_with_payload(payload);
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("bad sig"));
    }

    // ===================== C1 binding regression tests =====================

    #[tokio::test]
    async fn c1_rejects_authorization_recipient_mismatch() {
        // Attacker pairs a legitimate challenge (recipient=0xPayee) with an
        // EIP-3009 sig that pays a different address (e.g. attacker's own).
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock.clone());
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xAttacker", "value": "1000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0xn", "signature": "0xsig"
            }
        });
        let credential = credential_with_payload(payload);
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("authorization.to"), "got: {msg}");
        assert!(msg.contains("0xPayee"), "got: {msg}");
        // SA must NOT have been called.
        assert!(mock.last_settle.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn c1_rejects_authorization_value_mismatch() {
        // Cheap challenge (amount=1000) + sig for a much larger transfer.
        // Without binding, SA would broadcast and the larger transfer would
        // succeed, giving the attacker a paid resource for cheap.
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "1000000000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0xn", "signature": "0xsig"
            }
        });
        let credential = credential_with_payload(payload);
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("total"));
    }

    #[tokio::test]
    async fn c1_rejects_split_recipient_swap() {
        // Splits declared in challenge: ($50k → split1, $10k → split2).
        // Attacker swaps the order so funds go to attacker-controlled splits.
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let request = ChargeRequest {
            amount: "1000000".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            description: None,
            external_id: None,
            method_details: Some(
                serde_json::to_value(crate::types::ChargeMethodDetails {
                    chain_id: 196,
                    fee_payer: None,
                    permit2_address: None,
                    memo: None,
                    splits: Some(vec![
                        crate::types::ChargeSplit {
                            amount: "50000".into(),
                            recipient: "0xsplit1".into(),
                            memo: None,
                        },
                        crate::types::ChargeSplit {
                            amount: "10000".into(),
                            recipient: "0xsplit2".into(),
                            memo: None,
                        },
                    ]),
                })
                .unwrap(),
            ),
        };
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "940000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0x1111", "signature": "0xsig",
                "splits": [
                    // Swapped: 0xsplit2 first instead of 0xsplit1.
                    {
                        "from": "0xfrom", "to": "0xsplit2", "value": "50000",
                        "validAfter": "0", "validBefore": "9999999999",
                        "nonce": "0x2222", "signature": "0xs"
                    },
                    {
                        "from": "0xfrom", "to": "0xsplit1", "value": "10000",
                        "validAfter": "0", "validBefore": "9999999999",
                        "nonce": "0x3333", "signature": "0xs"
                    }
                ]
            }
        });
        let credential = credential_with_payload(payload);
        let err = method.verify(&credential, &request).await.unwrap_err();
        assert!(err.to_string().contains("splits[0].to"));
    }

    #[tokio::test]
    async fn c1_rejects_split_count_mismatch() {
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let request = ChargeRequest {
            amount: "1000".into(),
            currency: "0xToken".into(),
            decimals: None,
            recipient: Some("0xPayee".into()),
            description: None,
            external_id: None,
            method_details: None, // no splits in challenge
        };
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "500",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0x1", "signature": "0xs",
                "splits": [
                    { "from": "0xfrom", "to": "0xattacker", "value": "500",
                      "validAfter": "0", "validBefore": "9999999999",
                      "nonce": "0x2", "signature": "0xs" }
                ]
            }
        });
        let credential = credential_with_payload(payload);
        let err = method.verify(&credential, &request).await.unwrap_err();
        assert!(err.to_string().contains("authorization.splits present"));
    }

    #[tokio::test]
    async fn c1_h7_rejects_validbefore_below_challenge_expires() {
        // H7: validBefore must extend at least challenge.expires + 60s.
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let mut credential = credential_with_payload(serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "1000",
                "validAfter": "0",
                // 2030-01-01 = 1893456000 (roughly).
                "validBefore": "1893456000",
                "nonce": "0xn", "signature": "0xsig"
            }
        }));
        // Set challenge.expires AFTER validBefore (so binding fails H7).
        credential.challenge.expires = Some("2099-01-01T00:00:00Z".into());
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("validBefore"),
            "expected validBefore error, got: {err}"
        );
    }

    #[tokio::test]
    async fn c1_h7_accepts_validbefore_above_challenge_expires_plus_grace() {
        // Sanity: when validBefore > expires + 60s, binding passes.
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let mut credential = credential_with_payload(serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xPayee", "value": "1000",
                "validAfter": "0", "validBefore": "9999999999",
                "nonce": "0xn", "signature": "0xsig"
            }
        }));
        credential.challenge.expires = Some("2026-04-01T12:00:00Z".into());
        let receipt = method.verify(&credential, &dummy_request()).await.unwrap();
        assert_eq!(receipt.reference, "0xtxhash");
    }

    #[tokio::test]
    async fn c1_hash_mode_skips_binding() {
        // Hash mode has no `authorization` block — SA verifies the on-chain
        // tx itself. Binding intentionally skipped.
        let mock = Arc::new(MockSa::default());
        let method = EvmChargeMethod::new(mock);
        let payload = serde_json::json!({ "type": "hash", "hash": "0xchainhash" });
        let mut credential = credential_with_payload(payload);
        credential.source = Some("did:pkh:eip155:196:0xfrom".into());
        let receipt = method.verify(&credential, &dummy_request()).await.unwrap();
        assert_eq!(receipt.reference, "0xtxhash");
    }
}
