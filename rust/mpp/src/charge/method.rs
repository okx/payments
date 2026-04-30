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

const PAYLOAD_TYPE_TRANSACTION: &str = "transaction";
const PAYLOAD_TYPE_HASH: &str = "hash";

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
        _request: &ChargeRequest,
    ) -> impl Future<Output = Result<Receipt, VerificationError>> + Send {
        let sa_client = self.sa_client.clone();
        let credential = credential.clone();
        let challenge_id = credential.challenge.id.clone();

        async move {
            // Both `charge/settle` and `charge/verifyHash` need the
            // challenge object, so we forward the full credential
            // (challenge + payload + source) to SA API as-is.
            let credential_json = serde_json::to_value(&credential)
                .map_err(|e| VerificationError::new(format!("serialize credential: {}", e)))?;

            let payload_type = credential
                .payload
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

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
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xto", "value": "1000",
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
        // each entry carrying its own signature.
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0xfrom", "to": "0xpayee", "value": "940000",
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
        method.verify(&credential, &dummy_request()).await.unwrap();

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
        let payload = serde_json::json!({
            "type": "transaction",
            "authorization": { "type": "eip-3009" }
        });
        let credential = credential_with_payload(payload);
        let err = method
            .verify(&credential, &dummy_request())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("bad sig"));
    }
}
