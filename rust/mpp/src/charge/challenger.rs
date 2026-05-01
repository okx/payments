//! `EvmChargeChallenger` — EVM + SA API implementation of the upstream
//! `mpp::server::axum::ChargeChallenger` trait. Lets axum handlers use
//! `MppCharge<C>` extractors and `WithReceipt<T>` response wrappers with
//! zero boilerplate.
//!
//! Upstream ships `impl ChargeChallenger for Mpp<TempoChargeMethod<P>, S>`
//! and `impl ChargeChallenger for Mpp<StripeChargeMethod, S>`, but neither
//! covers the OKX X Layer + SA API path, so we provide our own impl.
//!
//! Upstream documents this extension hook explicitly
//! (`src/server/axum.rs:239-240`):
//!
//! > Implemented automatically for `Mpp<TempoChargeMethod<P>, S>` when the
//! > `tempo` feature is enabled. **Can also be implemented manually for
//! > custom payment methods.**
//!
//! # Implementation notes
//!
//! - **Holds an internal `Mpp<EvmChargeMethod>`.** `verify_payment`
//!   delegates to `Mpp::verify_credential`, which automatically does HMAC
//!   verification (preventing challenge_id forgery) and expiry checks —
//!   the same security guarantees as upstream Tempo / Stripe.
//! - **EVM-specific fields stored separately.** `currency / recipient /
//!   chain_id / fee_payer` are EVM-backend service-level configuration.
//!   The fields with the same names on upstream `Mpp<M>` exist for the
//!   Tempo helpers (`Mpp::charge()` and friends), which the EVM backend
//!   does not use, so we keep our own copies here.
//! - **`secret_key` is duplicated for challenge signing.** Upstream
//!   `Mpp<M>.secret_key` has no public getter, but we need one to sign
//!   challenges. The constructor writes it to both places to keep them in
//!   sync.
//!
//! # `amount` unit conventions
//!
//! `ChargeConfig::amount()`'s string is forwarded directly to
//! `ChargeRequest.amount` and **must be a base-units integer string** —
//! MPP protocol spec hard requirement:
//!
//! > `amount` MUST be a base-10 integer string with no sign, decimal point,
//! > exponent, or surrounding whitespace.
//!
//! Example: pathUSD (6 decimals) `0.01 pathUSD` is `"10000"`, not `"0.01"`.
//!
//! Upstream mpp-rs doc examples show `"0.01"` / `"1.00"`, but that's a
//! Tempo-backend convention (`TempoChargeMethod::charge_with_options`
//! internally converts dollars to base units). The protocol spec itself
//! disallows decimal points. This EVM challenger does not convert — it
//! forwards the string as-is — so **always pass base units**.
//!
//! # Design notes (per spec §3 #5)
//!
//! - **Global state lives on `EvmChargeChallenger`**: `currency` /
//!   `recipient` / `chain_id` / `fee_payer` / `realm` / `secret_key` are
//!   service-level parameters that don't vary across routes, set once at
//!   construction.
//! - **Per-route parameters come through the `ChargeConfig` trait**:
//!   `amount` and `description` are defined by each route's
//!   `impl ChargeConfig for OneCent`, passed to `challenge()` on every
//!   request by `MppCharge<C>`.
//! - One `EvmChargeChallenger` instance serves all MPP routes.
//!
//! # Usage
//!
//! ## Struct-literal style
//!
//! ```no_run
//! use std::sync::Arc;
//! use mpp::server::axum::ChargeChallenger;
//! use mpp_evm::{EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, OkxSaApiClient};
//!
//! let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
//! let challenger = EvmChargeChallenger::new(EvmChargeChallengerConfig {
//!     charge_method: EvmChargeMethod::new(sa),
//!     currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
//!     recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
//!     chain_id: 196,
//!     fee_payer: Some(true),
//!     realm: "photo.test".into(),
//!     secret_key: "hmac-secret".into(),
//!     splits: None,
//! });
//! let _: Arc<dyn ChargeChallenger> = Arc::new(challenger);
//! ```
//!
//! ## Builder style (matches upstream `Mpp::new(..).with_session_method(..)`)
//!
//! ```no_run
//! use std::sync::Arc;
//! use mpp_evm::{EvmChargeChallenger, EvmChargeMethod, OkxSaApiClient};
//!
//! let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
//! let challenger = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "photo.test", "hmac-secret")
//!     .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
//!     .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
//!     .chain_id(196)
//!     .fee_payer(true)
//!     .build();
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mpp::protocol::core::{parse_authorization, PaymentChallenge, Receipt};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger};
use mpp::server::Mpp;

use super::challenge::{build_charge_challenge, charge_request_with};
use super::method::EvmChargeMethod;
use crate::types::{ChargeMethodDetails, ChargeSplit};

/// Configuration for `EvmChargeChallenger` (struct-literal style).
///
/// All fields are **service-level parameters shared across routes**.
/// Per-route `amount` / `description` belong in the user-defined
/// `impl ChargeConfig for C`, not here.
pub struct EvmChargeChallengerConfig {
    /// EVM method instance, holding the SA API client (typically `OkxSaApiClient`).
    pub charge_method: EvmChargeMethod,
    /// ERC-20 contract address (the payment token), 40-hex.
    pub currency: String,
    /// Recipient address, 40-hex.
    pub recipient: String,
    /// Chain ID (X Layer = 196).
    pub chain_id: u64,
    /// Whether the server pays gas. `None` means unset (upstream default).
    pub fee_payer: Option<bool>,
    /// MPP auth realm (used in `WWW-Authenticate: Payment realm=...`).
    pub realm: String,
    /// HMAC key for signing challenge ids; the server must use the same value consistently.
    pub secret_key: String,
    /// Splits list (spec §8.1 `ChargeMethodDetails.splits`).
    ///
    /// Each entry is `ChargeSplit { amount, recipient, memo }` with the
    /// amount as a base-units integer string. Constraints:
    /// `sum(splits[].amount) < request.amount`, and the primary
    /// `recipient` must keep a non-zero balance (spec hard requirement;
    /// SA API enforces it). `None` disables splits.
    pub splits: Option<Vec<ChargeSplit>>,
}

/// `ChargeChallenger` implementation for the EVM backend + SA API.
///
/// See the module-level docs for usage.
#[derive(Clone)]
pub struct EvmChargeChallenger {
    inner: Arc<Inner>,
}

struct Inner {
    /// Upstream `Mpp<M>`, which provides HMAC + expiry verification
    /// (`verify_credential`) and stores realm / secret_key.
    mpp: Mpp<EvmChargeMethod>,
    /// EVM-specific service-level configuration.
    currency: String,
    recipient: String,
    chain_id: u64,
    fee_payer: Option<bool>,
    /// **Duplicate** `secret_key` used to sign challenges (upstream
    /// `Mpp::secret_key` has no public getter). The constructor keeps it
    /// in sync with the one inside `mpp`.
    secret_key: String,
    /// **Duplicate** `realm` for challenge signing (also available via
    /// `mpp.realm()`; a local copy saves a method call and improves readability).
    realm: String,
    /// Splits list (service-level config, shared across routes).
    /// `None` / empty `Vec` both disable splits.
    splits: Option<Vec<ChargeSplit>>,
}

impl EvmChargeChallenger {
    /// Struct-literal style constructor.
    pub fn new(cfg: EvmChargeChallengerConfig) -> Self {
        let mpp = Mpp::new(cfg.charge_method, cfg.realm.clone(), cfg.secret_key.clone());
        Self {
            inner: Arc::new(Inner {
                mpp,
                currency: cfg.currency,
                recipient: cfg.recipient,
                chain_id: cfg.chain_id,
                fee_payer: cfg.fee_payer,
                secret_key: cfg.secret_key,
                realm: cfg.realm,
                splits: cfg.splits,
            }),
        }
    }

    /// Chained builder constructor, matching upstream
    /// `Mpp::new(..).with_session_method(..)` style.
    pub fn builder(
        charge_method: EvmChargeMethod,
        realm: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> EvmChargeChallengerBuilder {
        EvmChargeChallengerBuilder {
            charge_method,
            realm: realm.into(),
            secret_key: secret_key.into(),
            currency: None,
            recipient: None,
            chain_id: None,
            fee_payer: None,
            splits: None,
        }
    }
}

/// Chained builder for `EvmChargeChallenger`.
///
/// **Required**: `charge_method / realm / secret_key` via the `builder()`
/// arguments; `currency / recipient / chain_id` via chained setters.
/// `fee_payer` is optional.
pub struct EvmChargeChallengerBuilder {
    charge_method: EvmChargeMethod,
    realm: String,
    secret_key: String,
    currency: Option<String>,
    recipient: Option<String>,
    chain_id: Option<u64>,
    fee_payer: Option<bool>,
    splits: Option<Vec<ChargeSplit>>,
}

impl EvmChargeChallengerBuilder {
    pub fn currency(mut self, v: impl Into<String>) -> Self {
        self.currency = Some(v.into());
        self
    }
    pub fn recipient(mut self, v: impl Into<String>) -> Self {
        self.recipient = Some(v.into());
        self
    }
    pub fn chain_id(mut self, v: u64) -> Self {
        self.chain_id = Some(v);
        self
    }
    pub fn fee_payer(mut self, v: bool) -> Self {
        self.fee_payer = Some(v);
        self
    }
    /// Splits list (spec §8.1). An empty `Vec` is normalized to `None`
    /// so we never send an empty array to SA API.
    pub fn splits(mut self, v: Vec<ChargeSplit>) -> Self {
        self.splits = if v.is_empty() { None } else { Some(v) };
        self
    }

    /// Finalize. Panics if `currency / recipient / chain_id` aren't set
    /// (missing a required field is a programmer error).
    pub fn build(self) -> EvmChargeChallenger {
        EvmChargeChallenger::new(EvmChargeChallengerConfig {
            charge_method: self.charge_method,
            realm: self.realm,
            secret_key: self.secret_key,
            currency: self.currency.expect("EvmChargeChallengerBuilder: currency() is required"),
            recipient: self
                .recipient
                .expect("EvmChargeChallengerBuilder: recipient() is required"),
            chain_id: self.chain_id.expect("EvmChargeChallengerBuilder: chain_id() is required"),
            fee_payer: self.fee_payer,
            splits: self.splits,
        })
    }
}

impl ChargeChallenger for EvmChargeChallenger {
    /// Assemble a `PaymentChallenge` from the per-route `amount` and
    /// service-level state.
    ///
    /// Pipeline:
    /// 1. `ChargeMethodDetails { chain_id, fee_payer, ... }`        (EVM method-specific fields)
    /// 2. `charge_request_with(amount, currency, recipient, dtls)`  (`ChargeRequest`, with method_details JSON)
    /// 3. `build_charge_challenge(secret_key, realm, &request, expires=None, description)`
    fn challenge(
        &self,
        amount: &str,
        options: ChallengeOptions,
    ) -> Result<PaymentChallenge, String> {
        let details = ChargeMethodDetails {
            chain_id: self.inner.chain_id,
            fee_payer: self.inner.fee_payer,
            permit2_address: None,
            memo: None,
            splits: self.inner.splits.clone(),
        };
        let request = charge_request_with(
            amount,
            &self.inner.currency,
            &self.inner.recipient,
            details,
        )?;
        build_charge_challenge(
            &self.inner.secret_key,
            &self.inner.realm,
            &request,
            None,
            options.description,
        )
    }

    /// Parse the credential string → delegate to upstream
    /// `Mpp::verify_credential` (which **does HMAC verification + expiry
    /// check + decodes the ChargeRequest + calls method.verify
    /// automatically**) → return `Receipt`.
    ///
    /// Versus the earlier implementation that called `method.verify`
    /// directly, the HMAC check here **prevents clients from forging
    /// challenge_ids to bypass server-side issuance constraints**.
    /// Without it, an attacker could forge any challenge and submit a
    /// payment credential; SA API only checks the on-chain EIP-3009
    /// signature and ignores challenge_id, which leaves a replay risk.
    fn verify_payment(
        &self,
        credential_str: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Receipt, String>> + Send>> {
        // Sync phase: parse the Authorization header into a credential (challenge + payload).
        let credential = match parse_authorization(credential_str) {
            Ok(c) => c,
            Err(e) => {
                return Box::pin(std::future::ready(Err(format!(
                    "parse authorization: {e}"
                ))));
            }
        };
        // Async phase: Mpp::verify_credential runs verify_hmac_and_expiry → method.verify.
        let mpp = self.inner.mpp.clone();
        Box::pin(async move {
            mpp.verify_credential(&credential)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SaApiError;
    use crate::sa_client::SaApiClient;
    use crate::types::{
        ChannelStatus, ChargeReceipt, CloseRequestPayload, SessionReceipt, SettleRequestPayload,
    };
    use mpp::protocol::intents::ChargeRequest;

    /// Test-only SA stub. `charge_settle` returns a synthetic receipt
    /// (`reference` contains "MOCK" for easy asserts); other methods are
    /// `unreachable!()` since this file's tests don't exercise them.
    #[derive(Debug, Default)]
    struct StubSa;

    #[async_trait::async_trait]
    impl SaApiClient for StubSa {
        async fn charge_settle(
            &self,
            credential: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            let challenge_id = credential
                .get("challenge")
                .and_then(|c| c.get("id"))
                .and_then(|id| id.as_str())
                .unwrap_or("stub-challenge-id")
                .to_string();
            Ok(ChargeReceipt {
                method: "evm".into(),
                reference: "0xMOCK_TX_HASH_0000000000000000000000000000000000000000000000000000000000".into(),
                status: "success".into(),
                timestamp: "2026-04-29T00:00:00Z".into(),
                chain_id: 196,
                confirmations: Some(1),
                challenge_id: Some(challenge_id),
                external_id: None,
            })
        }
        async fn charge_verify_hash(
            &self,
            _: &serde_json::Value,
        ) -> Result<ChargeReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_open(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_top_up(
            &self,
            _: &serde_json::Value,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_settle(
            &self,
            _: &SettleRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_close(
            &self,
            _: &CloseRequestPayload,
        ) -> Result<SessionReceipt, SaApiError> {
            unreachable!()
        }
        async fn session_status(&self, _: &str) -> Result<ChannelStatus, SaApiError> {
            unreachable!()
        }
    }

    fn test_challenger() -> EvmChargeChallenger {
        EvmChargeChallenger::new(EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(Arc::new(StubSa)),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: "test.local".into(),
            secret_key: "test-secret".into(),
            splits: None,
        })
    }

    #[test]
    fn challenge_yields_payment_challenge_with_evm_method() {
        let c = test_challenger();
        let ch = c
            .challenge(
                "100",
                ChallengeOptions {
                    description: Some("test item"),
                },
            )
            .expect("challenge ok");
        assert_eq!(ch.method.as_str(), "evm");
        assert_eq!(ch.intent.as_str(), "charge");
        assert_eq!(ch.realm, "test.local");
        assert_eq!(ch.description.as_deref(), Some("test item"));
        let req: ChargeRequest = ch.request.decode().unwrap();
        assert_eq!(req.amount, "100");
        assert_eq!(req.currency, "0x74b7F16337b8972027F6196A17a631aC6dE26d22");
        assert_eq!(
            req.recipient.as_deref(),
            Some("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
        );
    }

    #[test]
    fn challenge_without_description() {
        let c = test_challenger();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .expect("ok");
        assert!(ch.description.is_none());
    }

    #[test]
    fn builder_yields_equivalent_challenger() {
        let sa = Arc::new(StubSa);
        let c = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "test.local", "test-secret")
            .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
            .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
            .chain_id(196)
            .fee_payer(true)
            .build();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .unwrap();
        assert_eq!(ch.realm, "test.local");
    }

    #[test]
    fn splits_flow_into_challenge_method_details() {
        // Service-level splits should flow through challenge() into
        // request.method_details.splits, so the client can sign an
        // EIP-3009 authorization for each split recipient.
        let sa = Arc::new(StubSa);
        let c = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "test.local", "test-secret")
            .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
            .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
            .chain_id(196)
            .splits(vec![
                ChargeSplit {
                    amount: "30".into(),
                    recipient: "0x1111111111111111111111111111111111111111".into(),
                    memo: Some("partner-a".into()),
                },
                ChargeSplit {
                    amount: "20".into(),
                    recipient: "0x2222222222222222222222222222222222222222".into(),
                    memo: None,
                },
            ])
            .build();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .expect("challenge ok");
        let req: ChargeRequest = ch.request.decode().unwrap();
        let details: ChargeMethodDetails =
            serde_json::from_value(req.method_details.clone().unwrap()).unwrap();
        let splits = details.splits.expect("splits populated");
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].amount, "30");
        assert_eq!(splits[0].recipient, "0x1111111111111111111111111111111111111111");
        assert_eq!(splits[0].memo.as_deref(), Some("partner-a"));
        assert_eq!(splits[1].amount, "20");
        assert!(splits[1].memo.is_none());
    }

    #[test]
    fn empty_splits_vec_is_normalized_to_none() {
        // Empty Vec must not produce an empty splits array on the wire to client / SA API.
        let sa = Arc::new(StubSa);
        let c = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "test.local", "test-secret")
            .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
            .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
            .chain_id(196)
            .splits(vec![])
            .build();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .unwrap();
        let req: ChargeRequest = ch.request.decode().unwrap();
        let details: ChargeMethodDetails =
            serde_json::from_value(req.method_details.clone().unwrap()).unwrap();
        assert!(details.splits.is_none());
    }

    #[tokio::test]
    async fn verify_bad_credential_returns_err() {
        let c = test_challenger();
        let err = c.verify_payment("not-a-payment-header").await.unwrap_err();
        assert!(err.contains("parse authorization"));
    }

    #[tokio::test]
    async fn verify_valid_mock_credential_returns_receipt() {
        let c = test_challenger();
        // Use the challenger itself to generate a challenge — the id is
        // signed with our secret_key, so HMAC verification succeeds.
        let ch = c
            .challenge(
                "100",
                ChallengeOptions {
                    description: None,
                },
            )
            .unwrap();

        // Authorization fields must match the challenge (recipient + amount)
        // for C1 binding to pass.
        let credential_json = serde_json::json!({
            "challenge": {
                "id": ch.id,
                "realm": ch.realm,
                "method": "evm",
                "intent": "charge",
                "request": ch.request.raw(),
                "expires": ch.expires,
            },
            "payload": {
                "type": "transaction",
                "authorization": {
                    "type": "eip-3009",
                    "from": "0xfrom",
                    "to": "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1",
                    "value": "100",
                    "validAfter": "0", "validBefore": "9999999999",
                    "nonce": "0x01", "signature": "0xsig"
                }
            }
        });
        let cred_str = serde_json::to_string(&credential_json).unwrap();
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cred_str.as_bytes())
        };
        let auth_header = format!("Payment {b64}");

        let receipt = c.verify_payment(&auth_header).await.expect("verify ok");
        assert_eq!(receipt.method.as_str(), "evm");
        assert!(receipt.reference.contains("MOCK"));
    }

    /// Security check: a forged challenge.id that bypasses HMAC signing
    /// must be rejected by verify.
    #[tokio::test]
    async fn verify_forged_challenge_id_is_rejected() {
        let c = test_challenger();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .unwrap();

        // Build a credential that keeps the challenge's
        // realm/method/intent/request/expires but swaps `id` for an
        // attacker-supplied value. Verify must detect the mismatch via HMAC.
        let forged = serde_json::json!({
            "challenge": {
                "id": "attacker-forged-challenge-id",
                "realm": ch.realm,
                "method": "evm",
                "intent": "charge",
                "request": ch.request.raw(),
                "expires": ch.expires,
            },
            "payload": {
                "type": "transaction",
                "authorization": { "type": "eip-3009" }
            }
        });
        let cred_str = serde_json::to_string(&forged).unwrap();
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cred_str.as_bytes())
        };
        let auth_header = format!("Payment {b64}");

        let err = c
            .verify_payment(&auth_header)
            .await
            .expect_err("forged challenge must be rejected by HMAC verify");
        // Upstream Mpp::verify_hmac_and_expiry surfaces "Challenge ID mismatch - not issued by this server".
        assert!(
            err.to_lowercase().contains("challenge id mismatch")
                || err.to_lowercase().contains("challenge_id")
                || err.to_lowercase().contains("not issued by this server"),
            "unexpected error message: {err}"
        );
    }
}
