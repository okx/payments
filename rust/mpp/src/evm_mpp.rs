//! [`EvmMpp`] — unified facade holding charge + (optional) session
//! methods under a single service-level `EvmConfig`.
//!
//! ```ignore
//! use std::sync::Arc;
//! use mpp_evm::{
//!     EvmCharge, EvmConfig, EvmMpp, EvmSessionMethod, OkxSaApiClient,
//! };
//!
//! let sa: Arc<dyn mpp_evm::SaApiClient> = Arc::new(OkxSaApiClient::new(
//!     "k".into(), "s".into(), "p".into()
//! ));
//! let signer = alloy_signer_local::PrivateKeySigner::random();
//!
//! let mpp = EvmMpp::builder(EvmConfig {
//!         chain_id: 196,
//!         recipient: "0x...".into(),
//!         secret_key: "hmac-secret".into(),
//!         realm: "shop".into(),
//!     })
//!     .with_charge(EvmCharge::new(sa.clone()).with_fee_payer(true))
//!     .with_session(EvmSessionMethod::new(sa).with_signer(signer))
//!     .build()
//!     .expect("charge method required");
//! ```

use std::sync::Arc;

use mpp::protocol::core::{parse_authorization, PaymentChallenge, Receipt};
use mpp::protocol::traits::VerificationError;
use mpp::server::{Mpp as UpstreamMpp, SessionVerifyResult};

use crate::charge::challenge::{
    build_charge_challenge, build_session_challenge, charge_request_with, session_request_with,
};
use crate::charge::method::EvmChargeMethod;
use crate::sa_client::SaApiClient;
use crate::session_method::EvmSessionMethod;
use crate::types::{ChargeMethodDetails, ChargeSplit, SessionMethodDetails, SessionSplit};

// ============================================================================
// Service-level config
// ============================================================================

/// Service-level configuration shared by every route on this `EvmMpp`.
/// Set once at construction.
#[derive(Clone)]
pub struct EvmConfig {
    /// EVM chain id (e.g. 196 = X Layer).
    pub chain_id: u64,
    /// On-chain payment recipient (EIP-55 hex).
    pub recipient: String,
    /// HMAC secret used to sign / verify challenge ids.
    pub secret_key: String,
    /// Auth realm rendered into `WWW-Authenticate: Payment realm=...`.
    pub realm: String,
}

// Manual Debug to keep secret_key out of logs / panic frames.
impl std::fmt::Debug for EvmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvmConfig")
            .field("chain_id", &self.chain_id)
            .field("recipient", &self.recipient)
            .field("secret_key", &"***")
            .field("realm", &self.realm)
            .finish()
    }
}

// ============================================================================
// Method-level config: charge
// ============================================================================

/// Method-level configuration for the EVM `charge` intent.
///
/// Carries the SA API client plus charge-specific switches that don't
/// vary by route: `fee_payer` (gas sponsorship), `splits` (fixed-amount
/// secondary recipients), `resource_url` (per-endpoint revenue tag),
/// `permit2_address`.
#[derive(Clone)]
pub struct EvmCharge {
    pub(crate) sa_client: Arc<dyn SaApiClient>,
    pub(crate) fee_payer: Option<bool>,
    pub(crate) splits: Option<Vec<ChargeSplit>>,
    pub(crate) resource_url: Option<String>,
    pub(crate) permit2_address: Option<String>,
}

impl EvmCharge {
    /// Construct with the minimum required dependency.
    pub fn new(sa_client: Arc<dyn SaApiClient>) -> Self {
        Self {
            sa_client,
            fee_payer: None,
            splits: None,
            resource_url: None,
            permit2_address: None,
        }
    }
    /// Set `feePayer`: when true, the server pays gas (client must use
    /// `type="transaction"` mode).
    pub fn with_fee_payer(mut self, v: bool) -> Self {
        self.fee_payer = Some(v);
        self
    }
    /// Set fixed-amount payment splits.
    pub fn with_splits(mut self, v: Vec<ChargeSplit>) -> Self {
        self.splits = Some(v);
        self
    }
    /// Set per-endpoint `resource_url` for revenue attribution.
    pub fn with_resource_url(mut self, v: impl Into<String>) -> Self {
        self.resource_url = Some(v.into());
        self
    }
    /// Set Permit2 contract address.
    pub fn with_permit2_address(mut self, v: impl Into<String>) -> Self {
        self.permit2_address = Some(v.into());
        self
    }
}

// ============================================================================
// Per-route params (supplied by the adapter on every call)
// ============================================================================

/// Per-route fields for [`EvmMpp::charge_challenge`].
#[derive(Debug, Clone)]
pub struct ChargeRouteParams<'a> {
    /// Payment amount as a base-units integer string.
    pub amount: &'a str,
    /// ERC-20 token contract address.
    pub currency: &'a str,
    /// Optional human-readable description.
    pub description: Option<&'a str>,
    /// Optional merchant-supplied external id (echoed back in receipts).
    pub external_id: Option<&'a str>,
}

/// Per-route fields for [`EvmMpp::session_challenge`].
#[derive(Debug, Clone)]
pub struct SessionRouteParams<'a> {
    pub amount: &'a str,
    pub currency: &'a str,
    pub description: Option<&'a str>,
    /// Reserved for API symmetry with [`ChargeRouteParams`]; currently
    /// has no effect because upstream `SessionRequest` has no slot for
    /// it. May be wired through if upstream adds the field.
    pub external_id: Option<&'a str>,
    pub unit_type: Option<&'a str>,
    pub suggested_deposit: Option<&'a str>,
    /// Per-route override for session `splits` (proportional bps).
    /// `None` ⇒ no splits.
    pub splits: Option<Vec<SessionSplit>>,
}

// ============================================================================
// Facade
// ============================================================================

/// Unified handle for EVM MPP charge + session intents.
#[derive(Clone)]
pub struct EvmMpp {
    cfg: EvmConfig,
    charge: EvmCharge,
    /// Held separately from `inner` so adapters can reach into the
    /// session method directly (e.g. for status / settle calls outside
    /// the verify path).
    session: Option<Arc<EvmSessionMethod>>,
    inner: EvmMppInner,
}

/// Internal: parameterized upstream `Mpp<M, S>` instance. We can't
/// have a single concrete type with `Option<SessionMethod>` because
/// the upstream API gates `verify_session` on `S: SessionMethod`, so
/// we branch on whether session was registered.
#[derive(Clone)]
enum EvmMppInner {
    ChargeOnly(Arc<UpstreamMpp<EvmChargeMethod, ()>>),
    Both(Arc<UpstreamMpp<EvmChargeMethod, EvmSessionMethod>>),
}

impl EvmMpp {
    /// Start a builder. `charge` is mandatory; `session` is optional.
    pub fn builder(cfg: EvmConfig) -> EvmMppBuilder {
        EvmMppBuilder::new(cfg)
    }

    /// Service-level config (read-only).
    pub fn config(&self) -> &EvmConfig {
        &self.cfg
    }

    /// Direct handle to the registered session method, if any. Useful
    /// when the merchant wants to call `submit_voucher` /
    /// `settle_with_authorization` outside the router flow.
    pub fn session_method(&self) -> Option<&Arc<EvmSessionMethod>> {
        self.session.as_ref()
    }

    // -------------------------------------------------------------------
    // Challenge build
    // -------------------------------------------------------------------

    /// Build a `method="evm"` charge challenge for a single route.
    pub fn charge_challenge(
        &self,
        params: ChargeRouteParams<'_>,
    ) -> Result<PaymentChallenge, String> {
        let details = ChargeMethodDetails {
            chain_id: self.cfg.chain_id,
            fee_payer: self.charge.fee_payer,
            permit2_address: self.charge.permit2_address.clone(),
            memo: None,
            splits: self.charge.splits.clone(),
            resource_url: self.charge.resource_url.clone(),
        };
        let mut request =
            charge_request_with(params.amount, params.currency, &self.cfg.recipient, details)?;
        request.description = params.description.map(|s| s.to_string());
        request.external_id = params.external_id.map(|s| s.to_string());
        build_charge_challenge(
            &self.cfg.secret_key,
            &self.cfg.realm,
            &request,
            None,
            params.description,
        )
    }

    /// Build a `method="evm"` session challenge for a single route.
    /// Returns an error if no session method was registered.
    pub fn session_challenge(
        &self,
        params: SessionRouteParams<'_>,
    ) -> Result<PaymentChallenge, String> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| "EvmMpp: session method not registered".to_string())?;

        // Method details: pull escrow / chain_id / min_voucher_delta from
        // the session method's existing configuration, then layer in the
        // per-route `splits`. fee_payer flows through method config if
        // session method recorded one.
        let mut details: SessionMethodDetails =
            match session.method_details_value() {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("EvmMpp: parse session method_details: {e}"))?,
                None => SessionMethodDetails {
                    chain_id: self.cfg.chain_id,
                    escrow_contract: crate::types::DEFAULT_ESCROW_CONTRACT.into(),
                    channel_id: None,
                    min_voucher_delta: None,
                    fee_payer: None,
                    splits: None,
                },
            };
        // Per-route splits override.
        if params.splits.is_some() {
            details.splits = params.splits;
        }
        // chain_id must agree between `EvmMpp.cfg` and the registered
        // session method. Silent reconciliation would let the same
        // `EvmSessionMethod` behave differently depending on whether it's
        // used directly or through `EvmMpp.session_challenge`.
        if details.chain_id != self.cfg.chain_id {
            return Err(format!(
                "EvmMpp: chain_id disagreement — EvmMpp.cfg.chain_id={} but \
                 session.method_details.chain_id={}; reconcile your config \
                 (set them to the same value, or build a separate EvmMpp per chain)",
                self.cfg.chain_id, details.chain_id,
            ));
        }

        let request = build_session_request(
            params.amount,
            params.currency,
            &self.cfg.recipient,
            params.external_id,
            params.unit_type,
            params.suggested_deposit,
            details,
        )?;
        build_session_challenge(
            &self.cfg.secret_key,
            &self.cfg.realm,
            &request,
            None,
            params.description,
        )
    }

    // -------------------------------------------------------------------
    // Verify
    // -------------------------------------------------------------------

    /// Verify a charge credential (HMAC + expiry + SA-API roundtrip).
    pub async fn verify_charge(&self, auth_header: &str) -> Result<Receipt, String> {
        let credential =
            parse_authorization(auth_header).map_err(|e| format!("parse authorization: {e}"))?;
        match &self.inner {
            EvmMppInner::ChargeOnly(m) => m
                .verify_credential(&credential)
                .await
                .map_err(|e| e.to_string()),
            EvmMppInner::Both(m) => m
                .verify_credential(&credential)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    /// Verify a session credential. Errors out when no session method
    /// was registered.
    pub async fn verify_session(
        &self,
        auth_header: &str,
    ) -> Result<SessionVerifyResult, String> {
        let credential =
            parse_authorization(auth_header).map_err(|e| format!("parse authorization: {e}"))?;
        match &self.inner {
            EvmMppInner::ChargeOnly(_) => {
                Err("EvmMpp: session method not registered".to_string())
            }
            EvmMppInner::Both(m) => m
                .verify_session(&credential)
                .await
                .map_err(|e: VerificationError| e.to_string()),
        }
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Fluent builder for [`EvmMpp`].
pub struct EvmMppBuilder {
    cfg: EvmConfig,
    charge: Option<EvmCharge>,
    session: Option<EvmSessionMethod>,
}

impl EvmMppBuilder {
    fn new(cfg: EvmConfig) -> Self {
        Self {
            cfg,
            charge: None,
            session: None,
        }
    }

    pub fn with_charge(mut self, charge: EvmCharge) -> Self {
        self.charge = Some(charge);
        self
    }

    pub fn with_session(mut self, session: EvmSessionMethod) -> Self {
        self.session = Some(session);
        self
    }

    pub fn build(self) -> Result<EvmMpp, String> {
        let charge = self
            .charge
            .ok_or_else(|| "EvmMpp.build: with_charge(...) is required".to_string())?;
        let charge_method = EvmChargeMethod::new(charge.sa_client.clone());
        let realm = self.cfg.realm.clone();
        let secret = self.cfg.secret_key.clone();

        let (inner, session_arc) = match self.session {
            Some(s) => {
                // Two handles share all mutable state via Arc-wrapped fields
                // (sa_client / store / channel_locks / voucher_deduct_results).
                // Any new non-Arc mutable field must preserve this invariant.
                let session_for_arc = s.clone();
                let upstream = UpstreamMpp::new(charge_method, realm, secret).with_session_method(s);
                (
                    EvmMppInner::Both(Arc::new(upstream)),
                    Some(Arc::new(session_for_arc)),
                )
            }
            None => {
                let upstream = UpstreamMpp::new(charge_method, realm, secret);
                (EvmMppInner::ChargeOnly(Arc::new(upstream)), None)
            }
        };

        Ok(EvmMpp {
            cfg: self.cfg,
            charge,
            session: session_arc,
            inner,
        })
    }
}

// ============================================================================
// Helpers
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn build_session_request(
    amount: &str,
    currency: &str,
    recipient: &str,
    _external_id: Option<&str>,
    unit_type: Option<&str>,
    suggested_deposit: Option<&str>,
    details: SessionMethodDetails,
) -> Result<mpp::protocol::intents::SessionRequest, String> {
    let mut req = session_request_with(amount, currency, recipient, details)?;
    // `external_id` is accepted for API symmetry with `ChargeRouteParams`
    // but discarded — upstream `SessionRequest` has no slot for it.
    req.unit_type = unit_type.map(|s| s.to_string());
    req.suggested_deposit = suggested_deposit.map(|s| s.to_string());
    Ok(req)
}
